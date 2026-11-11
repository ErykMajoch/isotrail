use isotrail_bundle::{Landcover, SceneBundle, line_mask};

use super::season::Seasonal;
use super::{blend, hash, landcover_rgba};

const GLINT_KEY: u64 = 1 << 32;
const GLINT_PER_MILLE: u64 = 30;
const FOAM_KEY: u64 = 2 << 32;
/// Foam on the sea's cells against the land, and on the ring beyond them
const FOAM_PERCENT: [u64; 2] = [80, 30];
const PATH_BLEND: f32 = 0.45;
const ROAD_BLEND: f32 = 0.6;

const SHORE: [u8; 4] = [104, 166, 226, 255];
/// Darker than lakes, with surf of foam and shallow lake blue along its shore
const SEA: [u8; 4] = [38, 84, 158, 255];
const FOAM: [u8; 4] = [236, 240, 236, 255];
const GLINT: [u8; 4] = [214, 236, 250, 255];
const TRODDEN: [u8; 4] = [226, 204, 160, 255];
const PAVED: [u8; 4] = [196, 194, 190, 255];

/// Each cell's top colour: its ground in the season's colours, with lighter
/// lake shores, a darker sea with surf along its shore, rare glints on open
/// water or cracks in ice, and the line mask's paths, roads and streams
pub(super) fn top_colours(bundle: &SceneBundle, seasonal: &Seasonal) -> Vec<[u8; 4]> {
    let (grid, terrain) = (&bundle.grid, &bundle.terrain);
    let ground = seasonal.ground(bundle);
    let (cols, rows) = (grid.cols as usize, grid.rows as usize);
    let water = u8::from(Landcover::Water);
    let mut lake = vec![false; grid.cells()];
    for &cell in bundle.water.iter().flat_map(|body| &body.cells) {
        if let Some(lake) = lake.get_mut(cell as usize) {
            *lake = true;
        }
    }
    // Past the slab's edge is the cut face, not a shore
    let wet = |col: usize, row: usize, dc: isize, dr: isize| match (
        col.checked_add_signed(dc),
        row.checked_add_signed(dr),
    ) {
        (Some(c), Some(r)) if c < cols && r < rows => terrain.landcover[r * cols + c] == water,
        _ => true,
    };

    (0..grid.cells())
        .map(|cell| {
            let class = terrain.landcover[cell];
            let (col, row) = (cell % cols, cell / cols);
            if class == water {
                if !lake[cell] {
                    return landcover_rgba(class);
                }
                let around = |offsets: [(isize, isize); 4]| {
                    offsets.iter().all(|&(dc, dr)| wet(col, row, dc, dr))
                };
                let shore = !around([(1, 0), (-1, 0), (0, 1), (0, -1)]);
                if seasonal.frozen(cell) {
                    return if shore {
                        ground[cell]
                    } else {
                        seasonal.ice(cell)
                    };
                }
                if seasonal.sea(cell) {
                    let land_within = |reach: isize| {
                        (-reach..=reach).any(|dr| (-reach..=reach).any(|dc| !wet(col, row, dc, dr)))
                    };
                    let roll = hash(bundle.prop_seed, [FOAM_KEY, cell as u64]) % 100;
                    if let Some(ring) = (1..=2).find(|&reach| land_within(reach)) {
                        return if roll < FOAM_PERCENT[ring as usize - 1] {
                            FOAM
                        } else {
                            landcover_rgba(class)
                        };
                    }
                }
                let sea = seasonal.sea(cell);
                if shore {
                    return SHORE;
                }
                let open = around([(1, 1), (1, -1), (-1, 1), (-1, -1)]);
                if open && hash(bundle.prop_seed, [GLINT_KEY, cell as u64]) % 1000 < GLINT_PER_MILLE
                {
                    return GLINT;
                }
                return if sea { SEA } else { landcover_rgba(class) };
            }
            let ground = ground[cell];
            let lines = terrain.lines.get(cell).copied().unwrap_or(0);
            if lines & line_mask::PATH != 0 {
                blend(ground, TRODDEN, PATH_BLEND)
            } else if lines & line_mask::ROAD != 0 {
                blend(ground, PAVED, ROAD_BLEND)
            } else if lines & line_mask::STREAM != 0 {
                landcover_rgba(water)
            } else {
                ground
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::WaterBody;
    use isotrail_testkit::{bundle, grid};

    use super::*;

    fn rgba(class: Landcover) -> [u8; 4] {
        landcover_rgba(u8::from(class))
    }

    fn tops(bundle: &SceneBundle) -> Vec<[u8; 4]> {
        top_colours(bundle, &Seasonal::new(bundle))
    }

    /// A 12 × 12 lake with a 2-cell dry rim
    fn lake(seed: u64) -> (SceneBundle, Vec<[u8; 4]>) {
        let mut classes = vec![Landcover::Grassland; 16 * 16];
        for row in 2..14 {
            classes[row * 16 + 2..row * 16 + 14].fill(Landcover::Water);
        }
        let mut bundle = bundle(grid(16, 16, 10.0), &[50.0; 256], &classes);
        bundle.prop_seed = seed;
        let colours = tops(&bundle);
        (bundle, colours)
    }

    #[test]
    fn lakes_have_a_lighter_shore_band() {
        let (_, colours) = lake(1);
        for row in 0..16 {
            for col in 0..16 {
                let colour = colours[row * 16 + col];
                let ring = (2..14).contains(&row)
                    && (2..14).contains(&col)
                    && [row, col].iter().any(|&v| v == 2 || v == 13);
                if ring {
                    assert_eq!(colour, SHORE, "({col}, {row})");
                } else if !(3..13).contains(&row) || !(3..13).contains(&col) {
                    assert_eq!(colour, rgba(Landcover::Grassland), "({col}, {row})");
                } else {
                    assert_ne!(colour, SHORE, "({col}, {row})");
                }
            }
        }
    }

    #[test]
    fn glints_are_rare_seeded_and_off_the_shore() {
        let glints = |seed| {
            let (_, colours) = lake(seed);
            colours
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == GLINT)
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };
        let all: Vec<_> = (0..40).flat_map(glints).collect();
        // 10 × 10 open cells per lake, 3% of them
        let share = all.len() as f32 / (40.0 * 100.0);
        assert!((0.015..0.05).contains(&share), "{share}");
        for cell in all {
            let (col, row) = (cell % 16, cell / 16);
            assert!((3..13).contains(&col) && (3..13).contains(&row), "{cell}");
        }
        assert_eq!(glints(3), glints(3));
        let seeds = |range: std::ops::Range<u64>| range.map(glints).collect::<Vec<_>>();
        assert_ne!(seeds(0..5), seeds(5..10));
    }

    #[test]
    fn the_sea_is_darker_with_surf_along_its_shore() {
        let surf = |seed| {
            let (mut bundle, _) = lake(seed);
            bundle.water[0].sea = true;
            tops(&bundle)
        };
        let water = rgba(Landcover::Water);
        let mut foam = [0usize; 2];
        for seed in 0..20 {
            let colours = surf(seed);
            for row in 2..14usize {
                for col in 2..14usize {
                    let colour = colours[row * 16 + col];
                    let from_land = [row - 1, 14 - row, col - 1, 14 - col].into_iter().min();
                    match from_land {
                        Some(ring @ (1 | 2)) => {
                            assert!([FOAM, water].contains(&colour), "{col},{row}");
                            foam[ring - 1] += usize::from(colour == FOAM);
                        }
                        _ => assert!([SEA, GLINT].contains(&colour), "{col},{row}"),
                    }
                }
            }
        }
        // 44 cells in the first ring and 36 in the second, over 20 seeds
        let (near, far) = (foam[0] as f32 / 880.0, foam[1] as f32 / 720.0);
        assert!(
            (0.7..0.9).contains(&near) && (0.2..0.4).contains(&far),
            "{near} {far}"
        );
        assert_eq!(surf(3), surf(3));
        let lightness = |c: [u8; 4]| c[..3].iter().map(|&v| u32::from(v)).sum::<u32>();
        assert!(lightness(SEA) < lightness(water) && lightness(water) < lightness(FOAM));
    }

    #[test]
    fn rivers_keep_the_water_colour() {
        let (mut bundle, _) = lake(1);
        bundle.water.clear();
        let colours = tops(&bundle);
        assert_eq!(colours[2 * 16 + 2], rgba(Landcover::Water));
        assert_eq!(colours[7 * 16 + 7], rgba(Landcover::Water));
        assert!(!colours.contains(&SHORE) && !colours.contains(&GLINT));
    }

    #[test]
    fn lines_tint_the_ground_in_order() {
        use line_mask::{PATH, ROAD, STREAM};
        let classes = [Landcover::Heath; 6];
        let mut bundle = bundle(grid(6, 1, 10.0), &[0.0; 6], &classes);
        bundle.terrain.lines = vec![0, PATH, ROAD, STREAM, PATH | STREAM, ROAD | STREAM];
        let colours = tops(&bundle);
        let heath = rgba(Landcover::Heath);
        let trodden = blend(heath, TRODDEN, PATH_BLEND);
        let paved = blend(heath, PAVED, ROAD_BLEND);
        assert_eq!(
            colours,
            [
                heath,
                trodden,
                paved,
                rgba(Landcover::Water),
                trodden,
                paved
            ]
        );
        let lighter = |c: [u8; 4]| c[..3].iter().map(|&v| u32::from(v)).sum::<u32>();
        assert!(lighter(trodden) > lighter(heath) && lighter(paved) > lighter(heath));
    }

    #[test]
    fn missing_lines_and_stray_water_cells_are_ignored() {
        let mut bundle = bundle(grid(2, 1, 10.0), &[0.0; 2], &[Landcover::Rock; 2]);
        bundle.terrain.lines.clear();
        bundle.water.push(WaterBody {
            level_m: 0.0,
            cells: vec![7],
            sea: false,
        });
        assert_eq!(tops(&bundle), [rgba(Landcover::Rock); 2]);
    }
}
