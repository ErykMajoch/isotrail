use isotrail_bundle::{Landcover, SceneBundle};

use super::{Columns, Mesher, TerrainMesh, landcover_rgba, slab_position, strata};

/// A height at every cell corner and two flat-shaded triangles per cell,
/// coloured by the cell's landcover, over layered slab sides
#[derive(Clone, Copy, Debug, Default)]
pub struct SmoothMesher;

impl Mesher for SmoothMesher {
    fn mesh(&self, bundle: &SceneBundle) -> TerrainMesh {
        let grid = &bundle.grid;
        let (cols, rows) = (grid.cols as usize, grid.rows as usize);
        let corners = corner_heights(bundle);
        let corner = |col: usize, row: usize| {
            slab_position(grid, col as f64, row as f64).extend(corners[row * (cols + 1) + col])
        };

        let mut mesh = TerrainMesh {
            vertices: Vec::with_capacity(cols * rows * 6),
        };
        for row in 0..rows {
            for col in 0..cols {
                let colour = landcover_rgba(bundle.terrain.landcover[row * cols + col]);
                let (top_left, top_right) = (corner(col, row), corner(col + 1, row));
                let (bottom_left, bottom_right) = (corner(col, row + 1), corner(col + 1, row + 1));
                mesh.push_triangle([bottom_left, bottom_right, top_right], colour);
                mesh.push_triangle([bottom_left, top_right, top_left], colour);
            }
        }
        let block = grid.cell_size_m as f32;
        let columns = Columns::new(grid, bundle.terrain.landcover.clone());
        strata::cut_faces(
            &columns,
            bundle.prop_seed,
            block,
            strata::base(block),
            &mut mesh,
            |_, (col, row)| corners[row * (cols + 1) + col],
        );
        mesh
    }
}

/// Exaggerated heights above the lowest terrain at the `(cols + 1) × (rows + 1)`
/// cell corners, row-major. A corner is the mean of the cells around it, except
/// next to water, where it takes the water's level so lakes stay flat
fn corner_heights(bundle: &SceneBundle) -> Vec<f32> {
    let (grid, terrain) = (&bundle.grid, &bundle.terrain);
    let (cols, rows) = (grid.cols as usize, grid.rows as usize);
    let exaggeration = grid.exaggeration as f32;
    let water = u8::from(Landcover::Water);

    let mut corners = Vec::with_capacity((cols + 1) * (rows + 1));
    for row in 0..=rows {
        for col in 0..=cols {
            let (mut sum, mut count, mut lowest_water) = (0.0, 0.0, f32::INFINITY);
            for r in row.saturating_sub(1)..(row + 1).min(rows) {
                for c in col.saturating_sub(1)..(col + 1).min(cols) {
                    let cell = r * cols + c;
                    let height = terrain.height_m(cell) - terrain.height_offset_m;
                    if terrain.landcover[cell] == water {
                        lowest_water = lowest_water.min(height);
                    }
                    sum += height;
                    count += 1.0;
                }
            }
            let height = if lowest_water.is_finite() {
                lowest_water
            } else {
                sum / count
            };
            corners.push(height * exaggeration);
        }
    }
    corners
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use isotrail_bundle::GridInfo;
    use isotrail_testkit::{bundle, grid};

    use super::*;
    use crate::mesh::strata::base;

    fn mesh(grid: GridInfo, heights: &[f32], classes: &[Landcover]) -> TerrainMesh {
        SmoothMesher.mesh(&bundle(grid, heights, classes))
    }

    /// The terrain's own triangles, without the slab sides
    fn surface(mesh: &TerrainMesh) -> impl Iterator<Item = &[crate::Vertex; 3]> {
        mesh.triangles().filter(|t| t[0].normal[2] > 0.1)
    }

    #[test]
    fn two_triangles_per_cell_cover_the_slab_over_its_base() {
        let mesh = mesh(grid(3, 2, 10.0), &[100.0; 6], &[Landcover::Grassland; 6]);
        assert_eq!(surface(&mesh).count(), 12);
        let (min, max) = mesh
            .positions()
            .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), p| {
                (min.min(p), max.max(p))
            });
        assert_eq!(min, Vec3::new(-15.0, -10.0, base(10.0)));
        assert_eq!(max, Vec3::new(15.0, 10.0, 0.0));
    }

    #[test]
    fn flat_terrain_faces_up_in_its_landcover_colour() {
        let classes = [
            Landcover::Rock,
            Landcover::Grassland,
            Landcover::Coniferous,
            Landcover::Snow,
        ];
        let mesh = mesh(grid(2, 2, 5.0), &[40.0; 4], &classes);
        assert_eq!(surface(&mesh).count(), 8);
        for triangle in surface(&mesh) {
            for vertex in triangle {
                assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);
            }
        }
        // Cells in row-major order, rows from the +y edge
        let cell_colour = |x: f32, y: f32| {
            surface(&mesh)
                .find(|t| {
                    let c = t.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / 3.0;
                    (c.x < 0.0) == (x < 0.0) && (c.y < 0.0) == (y < 0.0)
                })
                .unwrap()[0]
                .colour
        };
        let rgba = |class: Landcover| landcover_rgba(u8::from(class));
        assert_eq!(cell_colour(-1.0, 1.0), rgba(Landcover::Rock));
        assert_eq!(cell_colour(1.0, 1.0), rgba(Landcover::Grassland));
        assert_eq!(cell_colour(-1.0, -1.0), rgba(Landcover::Coniferous));
        assert_eq!(cell_colour(1.0, -1.0), rgba(Landcover::Snow));
    }

    #[test]
    fn slopes_face_downhill_and_heights_are_exaggerated() {
        // Rising towards +x
        let heights = [0.0, 10.0, 20.0, 0.0, 10.0, 20.0];
        let grid = GridInfo {
            exaggeration: 2.0,
            ..grid(3, 2, 10.0)
        };
        let mesh = mesh(grid, &heights, &[Landcover::Grassland; 6]);
        let top = mesh.positions().map(|p| p.z).fold(0.0, f32::max);
        assert!((top - 40.0).abs() < 1e-3, "{top}");
        assert_eq!(surface(&mesh).count(), 12);
        for triangle in surface(&mesh) {
            let n = Vec3::from(triangle[0].normal);
            assert!(n.z > 0.0 && n.x <= 0.0, "{n}");
            assert!((n.length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn lakes_stay_flat_at_their_level() {
        // A 4 × 3 hillside with a lake in the two middle cells of the middle row
        #[rustfmt::skip]
        let heights = [
            90.0, 80.0, 80.0, 90.0,
            70.0, 50.0, 50.0, 70.0,
            90.0, 80.0, 80.0, 90.0,
        ];
        let mut classes = [Landcover::Grassland; 12];
        classes[5] = Landcover::Water;
        classes[6] = Landcover::Water;
        let mesh = mesh(grid(4, 3, 10.0), &heights, &classes);
        let water = landcover_rgba(u8::from(Landcover::Water));
        let lake: Vec<_> = surface(&mesh).filter(|t| t[0].colour == water).collect();
        assert_eq!(lake.len(), 4);
        for vertex in lake.into_iter().flatten() {
            assert!(vertex.position[2].abs() < 1e-3, "{vertex:?}");
            assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);
        }
    }
}
