use glam::Vec2;
use isotrail_bundle::Landcover;

use super::{BASE_BLOCKS, Columns, Side, TerrainMesh, hash, side_rgba};

/// Block rows above the base that are deep stone
const DEEP_BLOCKS: u32 = 2;
const MAX_WATER_BLOCKS: u32 = 4;
const EPSILON: f32 = 1e-3;

const WATER: [u8; 4] = [44, 96, 176, 255];
const DIRT: [u8; 4] = [122, 86, 58, 255];
const PEBBLE: [u8; 4] = [166, 142, 112, 255];
const STONE: [u8; 4] = [124, 122, 128, 255];
const DEEP_STONE: [u8; 4] = [74, 76, 92, 255];
const COPPER: [u8; 4] = [178, 110, 72, 255];
const GOLD: [u8; 4] = [206, 182, 92, 255];
const CRYSTAL: [u8; 4] = [112, 168, 196, 255];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stratum {
    Water,
    Topsoil,
    Dirt,
    Stone,
    DeepStone,
}

/// Closes the slab with walls along its four sides, from `base` up to
/// `surface(column, corner)`, the surface height above an edge column's outer
/// corner, in layers of blocks `block` tall that follow the surface
pub(super) fn cut_faces(
    columns: &Columns,
    seed: u64,
    block: f32,
    base: f32,
    mesh: &mut TerrainMesh,
    surface: impl Fn(usize, (usize, usize)) -> f32,
) {
    let (cols, rows) = (columns.cols, columns.rows);
    let landcover = &columns.landcover;
    for side in Side::ALL {
        let cells = side.edge_cells(cols, rows);
        let water = water_blocks(
            &cells
                .iter()
                .map(|&(c, r)| landcover[r * cols + c] == u8::from(Landcover::Water))
                .collect::<Vec<_>>(),
        );
        for (i, (&(col, row), water)) in cells.iter().zip(water).enumerate() {
            let cell = row * cols + col;
            let rocky = matches!(
                Landcover::try_from(landcover[cell]),
                Ok(Landcover::Rock | Landcover::Scree)
            );
            let key = [side as u64, i as u64];
            let column = Column {
                dirt: if rocky {
                    0
                } else {
                    2 + (hash(seed, [key[0], key[1], u64::MAX]) % 2) as u32
                },
                water,
                topsoil: side_rgba(landcover[cell]),
            };
            let corners = side.wall(col, row);
            let heights = corners.map(|corner| surface(cell, corner));
            let wall = corners.map(|corner| columns.corner(corner));
            for (block_row, polygon) in blocks(heights, block, base) {
                let depth =
                    ((heights[0] + heights[1]) / 2.0 - base - (block_row + 1) as f32 * block)
                        / block;
                let depth = (depth - EPSILON).ceil().max(0.0) as u32;
                let stratum = column.stratum(depth, block_row);
                let noise = hash(seed, [key[0], key[1], u64::from(block_row)]);
                let colour = column.colour(stratum, noise);
                let point = |p: Vec2| wall[0].lerp(wall[1], p.x).extend(p.y);
                for k in 1..polygon.len() - 1 {
                    let triangle = [polygon[0], polygon[k], polygon[k + 1]].map(point);
                    if (triangle[1] - triangle[0])
                        .cross(triangle[2] - triangle[0])
                        .length_squared()
                        > f32::EPSILON
                    {
                        mesh.push_triangle(triangle, colour);
                    }
                }
            }
        }
    }
}

/// Height of the base of a slab whose lowest terrain is at zero
pub(super) fn base(block: f32) -> f32 {
    -(BASE_BLOCKS as f32) * block
}

struct Column {
    dirt: u32,
    water: u32,
    topsoil: [u8; 4],
}

impl Column {
    /// `depth` in blocks below the surface, `row` in blocks above the base
    fn stratum(&self, depth: u32, row: u32) -> Stratum {
        if depth < self.water {
            return Stratum::Water;
        }
        if row < DEEP_BLOCKS {
            return Stratum::DeepStone;
        }
        let depth = depth - self.water;
        if self.water == 0 {
            match depth {
                0 => Stratum::Topsoil,
                d if d <= self.dirt => Stratum::Dirt,
                _ => Stratum::Stone,
            }
        } else if depth < self.dirt {
            Stratum::Dirt
        } else {
            Stratum::Stone
        }
    }

    fn colour(&self, stratum: Stratum, noise: u64) -> [u8; 4] {
        match stratum {
            Stratum::Water => WATER,
            Stratum::Topsoil => self.topsoil,
            Stratum::Dirt => speckle(noise, 60, DIRT, &[PEBBLE]),
            Stratum::Stone => speckle(noise, 50, STONE, &[COPPER, GOLD]),
            Stratum::DeepStone => speckle(noise, 80, DEEP_STONE, &[CRYSTAL, COPPER]),
        }
    }
}

/// `base` most of the time, and one of `specks` in `per_mille` of blocks
fn speckle(noise: u64, per_mille: u64, base: [u8; 4], specks: &[[u8; 4]]) -> [u8; 4] {
    if noise % 1000 < per_mille {
        specks[(noise / 1000 % specks.len() as u64) as usize]
    } else {
        base
    }
}

/// How many blocks of water each edge cell shows: its distance along the
/// edge to the nearest dry cell, so lakes deepen away from their shores
fn water_blocks(water: &[bool]) -> Vec<u32> {
    let mut depth = vec![MAX_WATER_BLOCKS; water.len()];
    let mut run = MAX_WATER_BLOCKS;
    for (i, &wet) in water.iter().enumerate() {
        run = if wet {
            (run + 1).min(MAX_WATER_BLOCKS)
        } else {
            0
        };
        depth[i] = run;
    }
    run = MAX_WATER_BLOCKS;
    for (i, &wet) in water.iter().enumerate().rev() {
        run = if wet {
            (run + 1).min(MAX_WATER_BLOCKS)
        } else {
            0
        };
        depth[i] = depth[i].min(run);
    }
    depth
}

/// The blocks of a one-block-wide wall under a straight surface from
/// `heights[0]` to `heights[1]`, as block rows above the base and convex
/// polygons in (share along the wall, height), counter-clockwise
fn blocks(heights: [f32; 2], block: f32, base: f32) -> impl Iterator<Item = (u32, Vec<Vec2>)> {
    let top = heights[0].max(heights[1]);
    let count = ((top - base) / block - EPSILON).ceil().max(0.0) as u32;
    (0..count).map(move |row| {
        let (bottom, top) = (base + row as f32 * block, base + (row + 1) as f32 * block);
        let rectangle = [
            Vec2::new(0.0, bottom),
            Vec2::new(1.0, bottom),
            Vec2::new(1.0, top),
            Vec2::new(0.0, top),
        ];
        (row, clip_below(&rectangle, heights))
    })
}

/// Sutherland–Hodgman clip of a polygon to below the line from
/// (0, `heights[0]`) to (1, `heights[1]`)
fn clip_below(polygon: &[Vec2], heights: [f32; 2]) -> Vec<Vec2> {
    let above = |p: Vec2| heights[0] + p.x * (heights[1] - heights[0]) - p.y;
    let mut clipped = Vec::with_capacity(polygon.len() + 1);
    for (i, &p) in polygon.iter().enumerate() {
        let q = polygon[(i + 1) % polygon.len()];
        let (fp, fq) = (above(p), above(q));
        if fp >= 0.0 {
            clipped.push(p);
        }
        if (fp >= 0.0) != (fq >= 0.0) {
            clipped.push(p.lerp(q, fp / (fp - fq)));
        }
    }
    clipped
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use isotrail_testkit::grid;

    use super::*;

    fn column(dirt: u32, water: u32) -> Column {
        Column {
            dirt,
            water,
            topsoil: [0; 4],
        }
    }

    fn area(triangle: &[Vec3; 3]) -> f32 {
        (triangle[1] - triangle[0])
            .cross(triangle[2] - triangle[0])
            .length()
            / 2.0
    }

    fn corners(triangle: &[crate::Vertex; 3]) -> [Vec3; 3] {
        triangle.map(|v| Vec3::from(v.position))
    }

    #[test]
    fn layers_follow_the_surface_over_a_deep_band() {
        let dry = column(3, 0);
        let layers: Vec<_> = (0..6).map(|depth| dry.stratum(depth, 10)).collect();
        use Stratum::*;
        assert_eq!(layers, [Topsoil, Dirt, Dirt, Dirt, Stone, Stone]);
        assert_eq!(dry.stratum(9, 1), DeepStone);
        assert_eq!(dry.stratum(0, 0), DeepStone);
        assert_eq!(dry.stratum(4, 2), Stone);
        assert_eq!(column(0, 0).stratum(1, 10), Stone);

        let lake = column(2, 2);
        let layers: Vec<_> = (0..5).map(|depth| lake.stratum(depth, 10)).collect();
        assert_eq!(layers, [Water, Water, Dirt, Dirt, Stone]);
        assert_eq!(lake.stratum(1, 0), Water);
    }

    #[test]
    fn lakes_deepen_away_from_their_shores_along_the_edge() {
        let (f, t) = (false, true);
        assert_eq!(water_blocks(&[f, t, t, t, t, t, f]), [0, 1, 2, 3, 2, 1, 0]);
        assert_eq!(water_blocks(&[t, t, f, t]), [2, 1, 0, 1]);
        assert_eq!(water_blocks(&[f; 3]), [0; 3]);
        assert_eq!(water_blocks(&[t; 12])[6], MAX_WATER_BLOCKS);
    }

    #[test]
    fn walls_fill_the_space_between_base_and_surface() {
        let columns = Columns::new(&grid(4, 3, 10.0), vec![4; 12]);
        // A tilted plane that doesn't line up with the blocks
        let surface = |(c, r): (usize, usize)| 7.3 * c as f32 + 13.1 * r as f32;
        let base = base(10.0);
        let mut mesh = TerrainMesh::default();
        cut_faces(&columns, 1, 10.0, base, &mut mesh, |_, corner| {
            surface(corner)
        });

        let expected = |corners: &[(usize, usize)]| -> f32 {
            corners
                .windows(2)
                .map(|w| 10.0 * ((surface(w[0]) + surface(w[1])) / 2.0 - base))
                .sum()
        };
        let sides = [
            (Vec3::Y, expected(&[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)])),
            (
                -Vec3::Y,
                expected(&[(0, 3), (1, 3), (2, 3), (3, 3), (4, 3)]),
            ),
            (-Vec3::X, expected(&[(0, 0), (0, 1), (0, 2), (0, 3)])),
            (Vec3::X, expected(&[(4, 0), (4, 1), (4, 2), (4, 3)])),
        ];
        for (normal, expected) in sides {
            let actual: f32 = mesh
                .triangles()
                .filter(|t| Vec3::from(t[0].normal).dot(normal) > 0.999)
                .map(|t| area(&corners(t)))
                .sum();
            assert!(
                (actual - expected).abs() < 0.05,
                "{normal}: {actual} vs {expected}"
            );
        }
        for triangle in mesh.triangles() {
            let normal = Vec3::from(triangle[0].normal);
            assert!(
                sides.iter().any(|&(n, _)| normal.dot(n) > 0.999),
                "{normal}"
            );
        }
        assert!(mesh.positions().all(|p| p.z >= base - 1e-4));
    }

    #[test]
    fn edge_lakes_show_their_water_in_cross_section() {
        use Landcover::{Grassland, Water};
        let mut classes = [Grassland; 14];
        classes[1..6].fill(Water);
        let columns = Columns::new(&grid(7, 2, 10.0), classes.map(u8::from).to_vec());
        let mut mesh = TerrainMesh::default();
        cut_faces(&columns, 1, 10.0, base(10.0), &mut mesh, |_, _| 200.0);
        let water_blocks_at = |col: usize| {
            let x = (col as f32 - 3.0) * 10.0;
            mesh.triangles()
                .filter(|t| t[0].normal == [0.0, 1.0, 0.0] && t[0].colour == WATER)
                .filter(|t| (t.iter().map(|v| v.position[0]).sum::<f32>() / 3.0 - x).abs() < 5.0)
                .count()
                / 2
        };
        let blocks: Vec<_> = (0..7).map(water_blocks_at).collect();
        assert_eq!(blocks, [0, 1, 2, 3, 2, 1, 0]);
    }

    #[test]
    fn rare_specks_are_seeded() {
        let stone: Vec<_> = (0..20_000)
            .map(|i| column(2, 0).colour(Stratum::Stone, hash(7, [0, i, 3])))
            .collect();
        let specks = stone.iter().filter(|&&c| c != STONE).count() as f32 / 20_000.0;
        assert!((0.04..0.06).contains(&specks), "{specks}");
        assert!(stone.contains(&COPPER) && stone.contains(&GOLD));

        let again: Vec<_> = (0..20_000)
            .map(|i| column(2, 0).colour(Stratum::Stone, hash(7, [0, i, 3])))
            .collect();
        let other: Vec<_> = (0..20_000)
            .map(|i| column(2, 0).colour(Stratum::Stone, hash(8, [0, i, 3])))
            .collect();
        assert_eq!(stone, again);
        assert_ne!(stone, other);
    }
}
