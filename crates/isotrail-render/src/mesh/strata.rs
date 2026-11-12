use std::ops::ControlFlow;

use glam::Vec2;
use isotrail_bundle::Landcover;

use super::{Columns, Side, StrataStyle, Terrain, TerrainMesh, hash};

/// Block rows above the base that are deep stone
const DEEP_BLOCKS: u32 = 2;
const MAX_WATER_BLOCKS: u32 = 4;
const EPSILON: f32 = 1e-3;

pub(super) const WATER: [u8; 4] = [44, 96, 176, 255];
pub(super) const SEA_WATER: [u8; 4] = [30, 62, 122, 255];
pub(super) const DIRT: [u8; 4] = [122, 86, 58, 255];
pub(super) const PEBBLE: [u8; 4] = [166, 142, 112, 255];
pub(super) const STONE: [u8; 4] = [124, 122, 128, 255];
pub(super) const DEEP_STONE: [u8; 4] = [74, 76, 92, 255];
pub(super) const COPPER: [u8; 4] = [178, 110, 72, 255];
pub(super) const GOLD: [u8; 4] = [206, 182, 92, 255];
pub(super) const CRYSTAL: [u8; 4] = [112, 168, 196, 255];
/// Specks per thousand blocks of dirt, stone and deep stone
pub(super) const SPECKS_PER_MILLE: [u32; 3] = [60, 50, 80];

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
    walls(
        columns,
        seed,
        block,
        base,
        surface,
        |wall, heights, runs| {
            let point = |p: Vec2| wall[0].lerp(wall[1], p.x).extend(p.y);
            for run in runs {
                let polygon = clip_below(&run.rectangle(block, base), heights);
                for k in 1..polygon.len().saturating_sub(1) {
                    let triangle = [polygon[0], polygon[k], polygon[k + 1]].map(point);
                    if (triangle[1] - triangle[0])
                        .cross(triangle[2] - triangle[0])
                        .length_squared()
                        > f32::EPSILON
                    {
                        mesh.push_triangle(triangle, run.colour);
                    }
                }
            }
            ControlFlow::Continue(())
        },
    );
}

/// The most vertices [`cut_faces`] would make, counting no further than
/// just past `limit`
pub(super) fn vertex_bound(
    columns: &Columns,
    seed: u64,
    block: f32,
    base: f32,
    surface: impl Fn(usize, (usize, usize)) -> f32,
    limit: u64,
) -> u64 {
    let mut vertices = 0u64;
    walls(columns, seed, block, base, surface, |_, _, runs| {
        vertices += runs.len() as u64 * RUN_VERTICES;
        if vertices > limit {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    vertices
}

/// A rectangle cut below a line has at most five corners, so three triangles
const RUN_VERTICES: u64 = 9;

/// Hands each edge cell's wall to `each` as its two corners, the surface
/// heights above them and its runs of equal blocks
fn walls(
    columns: &Columns,
    seed: u64,
    block: f32,
    base: f32,
    surface: impl Fn(usize, (usize, usize)) -> f32,
    mut each: impl FnMut([Vec2; 2], [f32; 2], &[Run]) -> ControlFlow<()>,
) {
    let (cols, rows) = (columns.cols, columns.rows);
    let landcover = &columns.landcover;
    let mut runs = Vec::new();
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
                frozen: columns.frozen[cell],
                sea: columns.sea[cell],
                topsoil: columns.side(cell),
                ice: columns.style.water.ice,
            };
            let corners = side.wall(col, row);
            let heights = corners.map(|corner| surface(cell, corner));
            runs.clear();
            for block_row in 0..block_rows(heights, block, base) {
                let depth =
                    ((heights[0] + heights[1]) / 2.0 - base - (block_row + 1) as f32 * block)
                        / block;
                let depth = (depth - EPSILON).ceil().max(0.0) as u32;
                let stratum = column.stratum(depth, block_row);
                let noise = hash(seed, [key[0], key[1], u64::from(block_row)]);
                let style = &columns.style.strata;
                let colour = if columns.strata || stratum == Stratum::Water {
                    column.colour(stratum, depth, noise, style)
                } else {
                    style.stone
                };
                match runs.last_mut() {
                    Some(Run {
                        rows, colour: last, ..
                    }) if *last == colour => *rows += 1,
                    _ => runs.push(Run {
                        first: block_row,
                        rows: 1,
                        colour,
                    }),
                }
            }
            let wall = corners.map(|corner| columns.corner(corner));
            if each(wall, heights, &runs).is_break() {
                return;
            }
        }
    }
}

/// Height of the base of a slab whose lowest terrain is at zero
pub(super) fn base(block: f32, terrain: &Terrain) -> f32 {
    -(terrain.base_blocks as f32) * block
}

struct Column {
    dirt: u32,
    water: u32,
    frozen: bool,
    sea: bool,
    topsoil: [u8; 4],
    ice: [u8; 4],
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

    fn colour(&self, stratum: Stratum, depth: u32, noise: u64, s: &StrataStyle) -> [u8; 4] {
        let [dirt, stone, deep] = s.specks_per_mille.map(u64::from);
        match stratum {
            Stratum::Water if self.frozen && depth == 0 => self.ice,
            Stratum::Water if self.sea => s.sea_water,
            Stratum::Water => s.water,
            Stratum::Topsoil => self.topsoil,
            Stratum::Dirt => speckle(noise, dirt, s.dirt, &[s.pebble]),
            Stratum::Stone => speckle(noise, stone, s.stone, &[s.copper, s.gold]),
            Stratum::DeepStone => speckle(noise, deep, s.deep_stone, &[s.crystal, s.copper]),
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

/// Block rows stacked one above another in the same colour
#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    /// Block rows above the base
    first: u32,
    rows: u32,
    colour: [u8; 4],
}

impl Run {
    /// The run in (share along the wall, height), counter-clockwise
    fn rectangle(&self, block: f32, base: f32) -> [Vec2; 4] {
        let bottom = base + self.first as f32 * block;
        let top = base + (self.first + self.rows) as f32 * block;
        [
            Vec2::new(0.0, bottom),
            Vec2::new(1.0, bottom),
            Vec2::new(1.0, top),
            Vec2::new(0.0, top),
        ]
    }
}

/// How many block rows a one-block-wide wall under a straight surface from
/// `heights[0]` to `heights[1]` needs, counted from the base
fn block_rows(heights: [f32; 2], block: f32, base: f32) -> u32 {
    let top = heights[0].max(heights[1]);
    ((top - base) / block - EPSILON).ceil().max(0.0) as u32
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
            frozen: false,
            sea: false,
            topsoil: [0; 4],
            ice: ICE,
        }
    }

    const ICE: [u8; 4] = crate::mesh::season::ICE;

    fn colour(column: &Column, stratum: Stratum, depth: u32, noise: u64) -> [u8; 4] {
        column.colour(stratum, depth, noise, &StrataStyle::default())
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
        let base = base(10.0, &Terrain::default());
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
        cut_faces(
            &columns,
            1,
            10.0,
            base(10.0, &Terrain::default()),
            &mut mesh,
            |_, _| 200.0,
        );
        let water_blocks_at = |col: usize| {
            let x = (col as f32 - 3.0) * 10.0;
            let area: f32 = mesh
                .triangles()
                .filter(|t| t[0].normal == [0.0, 1.0, 0.0] && t[0].colour == WATER)
                .filter(|t| (t.iter().map(|v| v.position[0]).sum::<f32>() / 3.0 - x).abs() < 5.0)
                .map(|t| area(&corners(t)))
                .sum();
            (area / 100.0).round() as u32
        };
        let blocks: Vec<_> = (0..7).map(water_blocks_at).collect();
        assert_eq!(blocks, [0, 1, 2, 3, 2, 1, 0]);
    }

    /// Area of each colour on the walls when every block is drawn on its own
    fn block_by_block(
        columns: &Columns,
        block: f32,
        surface: impl Fn(usize, (usize, usize)) -> f32,
    ) -> std::collections::BTreeMap<[u8; 4], f32> {
        let mut areas = std::collections::BTreeMap::new();
        walls(
            columns,
            1,
            block,
            base(block, &Terrain::default()),
            surface,
            |wall, heights, runs| {
                let width = wall[0].distance(wall[1]);
                for run in runs {
                    for row in run.first..run.first + run.rows {
                        let one = Run {
                            first: row,
                            rows: 1,
                            ..*run
                        };
                        let polygon = clip_below(
                            &one.rectangle(block, base(block, &Terrain::default())),
                            heights,
                        );
                        let shoelace: f32 = (0..polygon.len())
                            .map(|i| polygon[i].perp_dot(polygon[(i + 1) % polygon.len()]))
                            .sum();
                        *areas.entry(run.colour).or_insert(0.0) += shoelace.abs() / 2.0 * width;
                    }
                }
                ControlFlow::Continue(())
            },
        );
        areas
    }

    #[test]
    fn equal_blocks_merge_into_one_face_covering_the_same_wall() {
        let classes: Vec<u8> = (0..32).map(|i| [4, 2, 0, 7][i % 4]).collect();
        let columns = Columns::new(&grid(8, 4, 10.0), classes);
        let surface = |(c, r): (usize, usize)| 4_000.0 + 37.3 * c as f32 - 113.1 * r as f32;
        let block = 10.0;
        let mut mesh = TerrainMesh::default();
        cut_faces(
            &columns,
            1,
            block,
            base(block, &Terrain::default()),
            &mut mesh,
            |_, corner| surface(corner),
        );

        let blocks: u64 = Side::ALL
            .into_iter()
            .flat_map(|side| {
                let cells = side.edge_cells(8, 4);
                cells.into_iter().map(move |(col, row)| side.wall(col, row))
            })
            .map(|corners| {
                u64::from(block_rows(
                    corners.map(surface),
                    block,
                    base(block, &Terrain::default()),
                ))
            })
            .sum();
        let vertices = mesh.vertices.len() as u64;
        assert!(
            vertices * 5 < blocks * 6,
            "{vertices} vertices for {blocks} blocks"
        );
        let bound = vertex_bound(
            &columns,
            1,
            block,
            base(block, &Terrain::default()),
            |_, c| surface(c),
            u64::MAX,
        );
        assert!(bound >= vertices, "{bound} < {vertices}");
        assert!(bound <= vertices * 2, "{bound} for {vertices}");

        let mut merged = std::collections::BTreeMap::new();
        for triangle in mesh.triangles() {
            *merged.entry(triangle[0].colour).or_insert(0.0) += area(&corners(triangle));
        }
        let separate = block_by_block(&columns, block, |_, c| surface(c));
        assert_eq!(
            merged.keys().collect::<Vec<_>>(),
            separate.keys().collect::<Vec<_>>()
        );
        for (colour, area) in separate {
            let relative = (merged[&colour] - area).abs() / area;
            assert!(relative < 1e-3, "{colour:?}: {} vs {area}", merged[&colour]);
        }
    }

    #[test]
    fn counting_stops_past_the_limit() {
        let columns = Columns::new(&grid(8, 4, 10.0), vec![4; 32]);
        let full = vertex_bound(
            &columns,
            1,
            1.0,
            base(1.0, &Terrain::default()),
            |_, _| 50_000.0,
            u64::MAX,
        );
        let capped = vertex_bound(
            &columns,
            1,
            1.0,
            base(1.0, &Terrain::default()),
            |_, _| 50_000.0,
            1_000,
        );
        assert!(capped > 1_000 && capped < full, "{capped} of {full}");
    }

    #[test]
    fn frozen_lakes_show_ice_over_their_water() {
        let frozen = Column {
            frozen: true,
            ..column(2, 3)
        };
        let colours = (0..3).map(|depth| colour(&frozen, frozen.stratum(depth, 5), depth, 0));
        assert_eq!(colours.collect::<Vec<_>>(), [ICE, WATER, WATER]);
        assert_eq!(colour(&column(2, 3), Stratum::Water, 0, 0), WATER);
    }

    #[test]
    fn the_sea_shows_darker_water() {
        use Stratum::{Dirt, Water};
        let sea = Column {
            sea: true,
            ..column(2, 3)
        };
        let layers = (0..4).map(|depth| sea.stratum(depth, 5));
        assert_eq!(layers.collect::<Vec<_>>(), [Water, Water, Water, Dirt]);
        assert_eq!(colour(&sea, Water, 0, 0), SEA_WATER);
    }

    #[test]
    fn without_strata_the_walls_are_plain_stone_under_any_water() {
        use Landcover::{Grassland, Water};
        let mut classes = [Grassland; 14];
        classes[1..6].fill(Water);
        let plain = Terrain {
            strata: false,
            ..Terrain::default()
        };
        let columns =
            Columns::new(&grid(7, 2, 10.0), classes.map(u8::from).to_vec()).shaped(&plain);
        let mut mesh = TerrainMesh::default();
        cut_faces(&columns, 1, 10.0, base(10.0, &plain), &mut mesh, |_, _| {
            200.0
        });
        let mut colours: Vec<[u8; 4]> = mesh.triangles().map(|t| t[0].colour).collect();
        colours.sort_unstable();
        colours.dedup();
        assert_eq!(colours, [WATER, STONE]);
        let deeper = Terrain {
            base_blocks: 12,
            ..plain
        };
        assert_eq!(base(10.0, &deeper), -120.0);
    }

    #[test]
    fn rare_specks_are_seeded() {
        let stone: Vec<_> = (0..20_000)
            .map(|i| colour(&column(2, 0), Stratum::Stone, 3, hash(7, [0, i, 3])))
            .collect();
        let specks = stone.iter().filter(|&&c| c != STONE).count() as f32 / 20_000.0;
        assert!((0.04..0.06).contains(&specks), "{specks}");
        assert!(stone.contains(&COPPER) && stone.contains(&GOLD));

        let again: Vec<_> = (0..20_000)
            .map(|i| colour(&column(2, 0), Stratum::Stone, 3, hash(7, [0, i, 3])))
            .collect();
        let other: Vec<_> = (0..20_000)
            .map(|i| colour(&column(2, 0), Stratum::Stone, 3, hash(8, [0, i, 3])))
            .collect();
        assert_eq!(stone, again);
        assert_ne!(stone, other);
    }
}
