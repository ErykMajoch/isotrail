use isotrail_bundle::SceneBundle;

use super::{Columns, Mesher, Side, Surface, TerrainMesh, landcover_rgba, side_rgba, strata};

/// One column per cell, its height rounded to whole blocks, with a top in the
/// cell's landcover colour and darker sides
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoxelMesher {
    /// Height of a block as a share of the cell size
    pub block_height: f32,
}

impl Default for VoxelMesher {
    fn default() -> VoxelMesher {
        VoxelMesher { block_height: 1.0 }
    }
}

impl Mesher for VoxelMesher {
    fn mesh(&self, bundle: &SceneBundle) -> TerrainMesh {
        let step = bundle.grid.cell_size_m as f32 * self.block_height;
        let tops = column_tops(bundle, step);
        let columns = Columns::new(&bundle.grid, bundle.terrain.landcover.clone());
        let lowest = tops.iter().copied().fold(f32::INFINITY, f32::min);
        let (cols, rows) = (columns.cols, columns.rows);
        let landcover = &columns.landcover;

        let mut mesh = TerrainMesh::default();
        for row in 0..rows {
            for col in 0..cols {
                let cell = row * cols + col;
                let top = tops[cell];
                let corner = |corner| columns.corner(corner).extend(top);
                mesh.push_quad(
                    [
                        corner((col, row + 1)),
                        corner((col + 1, row + 1)),
                        corner((col + 1, row)),
                        corner((col, row)),
                    ],
                    landcover_rgba(landcover[cell]),
                );
                for side in Side::ALL {
                    let Some((c, r)) = side.neighbour(col, row, cols, rows) else {
                        continue;
                    };
                    let below = tops[r * cols + c];
                    if below < top {
                        let wall = side.wall(col, row).map(|corner| columns.corner(corner));
                        mesh.push_wall(wall, below, top, side_rgba(landcover[cell]));
                    }
                }
            }
        }
        strata::cut_faces(
            &columns,
            bundle.prop_seed,
            step,
            lowest + strata::base(step),
            &mut mesh,
            |cell, _| tops[cell],
        );
        mesh.surface = Surface::Steps { cols, tops };
        mesh
    }
}

/// Exaggerated column heights above the lowest terrain, in whole steps
fn column_tops(bundle: &SceneBundle, step: f32) -> Vec<f32> {
    let (grid, terrain) = (&bundle.grid, &bundle.terrain);
    let exaggeration = grid.exaggeration as f32;
    (0..grid.cells())
        .map(|cell| {
            let height = (terrain.height_m(cell) - terrain.height_offset_m) * exaggeration;
            (height / step).round() * step
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use isotrail_bundle::{GridInfo, Landcover};
    use isotrail_testkit::{bundle, grid};

    use super::*;
    use crate::mesh::strata::base;

    fn mesh(grid: GridInfo, heights: &[f32], classes: &[Landcover]) -> TerrainMesh {
        VoxelMesher::default().mesh(&bundle(grid, heights, classes))
    }

    /// Triangles that face sideways and sit inside the slab, not on its edges
    fn inner_walls(mesh: &TerrainMesh, grid: &GridInfo) -> Vec<[Vec3; 3]> {
        let (u, v) = (grid.length_m() as f32 / 2.0, grid.width_m() as f32 / 2.0);
        mesh.triangles()
            .filter(|t| t[0].normal[2] == 0.0)
            .map(|t| t.map(|v| Vec3::from(v.position)))
            .filter(|t| {
                let on = |coord: fn(&Vec3) -> f32, half: f32| {
                    t.iter().all(|p| (coord(p).abs() - half).abs() < 1e-3)
                };
                !on(|p| p.x, u) && !on(|p| p.y, v)
            })
            .collect()
    }

    #[test]
    fn columns_rise_in_whole_blocks_from_a_base_below_the_terrain() {
        let heights = [103.0, 114.0, 126.0, 131.0, 109.0, 103.0];
        let grid = GridInfo {
            exaggeration: 2.0,
            ..grid(3, 2, 10.0)
        };
        let mesh = mesh(grid, &heights, &[Landcover::Grassland; 6]);
        for p in mesh.positions() {
            assert!((p.z / 10.0 - (p.z / 10.0).round()).abs() < 1e-4, "{p}");
        }
        let tops: Vec<f32> = mesh
            .triangles()
            .filter(|t| t[0].normal == [0.0, 0.0, 1.0])
            .map(|t| t[0].position[2])
            .collect();
        assert_eq!(tops.len(), 12);
        // (height − 103) × 2, rounded to tens
        for expected in [0.0, 20.0, 50.0, 60.0, 10.0] {
            assert!(tops.contains(&expected), "{expected} in {tops:?}");
        }
        let lowest = mesh.positions().map(|p| p.z).fold(f32::INFINITY, f32::min);
        assert_eq!(lowest, base(10.0));
        assert_eq!(lowest, -60.0);
    }

    #[test]
    fn level_neighbours_hide_their_shared_sides() {
        let grid = grid(3, 2, 10.0);
        let mesh = mesh(grid, &[50.0; 6], &[Landcover::Rock; 6]);
        assert!(inner_walls(&mesh, &grid).is_empty());
    }

    #[test]
    fn steps_show_one_darker_side_towards_the_lower_column() {
        let grid = grid(2, 1, 10.0);
        let classes = [Landcover::Grassland, Landcover::Coniferous];
        let mesh = mesh(grid, &[0.0, 20.0], &classes);
        let walls: Vec<_> = mesh
            .triangles()
            .filter(|t| t.iter().all(|v| v.position[0].abs() < 1e-4))
            .collect();
        assert_eq!(walls.len(), 2);
        for triangle in walls {
            assert_eq!(triangle[0].normal, [-1.0, 0.0, 0.0]);
            assert_eq!(
                triangle[0].colour,
                side_rgba(u8::from(Landcover::Coniferous))
            );
            for vertex in triangle {
                let z = vertex.position[2];
                assert!(z == 0.0 || z == 20.0, "{z}");
            }
        }
        let tops: Vec<_> = mesh
            .triangles()
            .filter(|t| t[0].normal == [0.0, 0.0, 1.0])
            .map(|t| t[0].colour)
            .collect();
        assert_eq!(tops[0], landcover_rgba(u8::from(Landcover::Grassland)));
        assert_eq!(tops[2], landcover_rgba(u8::from(Landcover::Coniferous)));
    }

    #[test]
    fn lakes_are_flat_tops() {
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
        let lake: Vec<_> = mesh.triangles().filter(|t| t[0].colour == water).collect();
        assert_eq!(lake.len(), 4);
        for vertex in lake.into_iter().flatten() {
            assert_eq!(vertex.position[2], 0.0);
            assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);
        }
    }

    #[test]
    fn block_height_sets_the_step() {
        let tops = |block_height: f32| {
            let mesh = VoxelMesher { block_height }.mesh(&bundle(
                grid(2, 1, 10.0),
                &[0.0, 13.0],
                &[Landcover::Rock; 2],
            ));
            let mut tops: Vec<f32> = mesh
                .triangles()
                .filter(|t| t[0].normal == [0.0, 0.0, 1.0])
                .map(|t| t[0].position[2])
                .collect();
            tops.dedup();
            let lowest = mesh.positions().map(|p| p.z).fold(f32::INFINITY, f32::min);
            (tops, lowest)
        };
        assert_eq!(tops(1.0), (vec![0.0, 10.0], -60.0));
        assert_eq!(tops(0.5), (vec![0.0, 15.0], -30.0));
        assert_eq!(tops(2.0), (vec![0.0, 20.0], -120.0));
    }
}
