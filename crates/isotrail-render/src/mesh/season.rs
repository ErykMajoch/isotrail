use isotrail_bundle::{Landcover, SceneBundle};

use super::{blend, hash, landcover_rgba};

const SNOW_KEY: u64 = 3 << 32;
const CELL_KEY: u64 = 4 << 32;
/// Below the snowline, snow lies in patches that thin out over this drop
const PATCHY_M: f32 = 150.0;
/// Snow slides off anything steeper
const SNOW_MAX_SLOPE_DEG: f32 = 50.0;
/// Plants run this many days behind in spring, and ahead in autumn, for
/// every metre above the slab's lowest ground
const DAYS_PER_M: f32 = 0.02;

pub(super) const SNOW: [u8; 4] = [236, 242, 248, 255];
pub(super) const ICE: [u8; 4] = [202, 224, 238, 255];
const CRACK: [u8; 4] = [150, 186, 214, 255];
const CRACK_PER_MILLE: u64 = 60;
pub(super) const FRESH: [u8; 4] = [168, 212, 90, 255];
const BLOSSOM: [[u8; 4]; 2] = [[240, 236, 228, 255], [232, 166, 190, 255]];
const AUTUMN: [[u8; 4]; 4] = [
    [226, 178, 48, 255],
    [218, 120, 36, 255],
    [156, 84, 40, 255],
    [150, 98, 50, 255],
];
const LITTER: [u8; 4] = [128, 98, 62, 255];
const FLOWERS: [[u8; 4]; 3] = [
    [236, 206, 82, 255],
    [240, 236, 224, 255],
    [226, 150, 180, 255],
];
const FLOWER_PER_MILLE: u64 = 70;
const YOUNG_CROP: [u8; 4] = [146, 196, 84, 255];
const GOLDEN: [u8; 4] = [200, 150, 60, 255];
const PLOUGHED: [u8; 4] = [168, 124, 80, 255];
const BRACKEN: [u8; 4] = [200, 176, 120, 255];
const DULL: [u8; 4] = [168, 124, 80, 255];
const DARK_HEATH: [u8; 4] = [92, 80, 66, 255];
/// Each cell takes between these shares of its ground's seasonal tint, so
/// the change mottles in rather than flipping the whole slab at once
const SPREAD: (f32, f32) = (0.8, 1.2);

/// Calendar of the trees, in days of the year as in the north at the
/// slab's lowest ground; each tree draws its own days within these spans
const LEAF_OUT: (f32, f32) = (95.0, 140.0);
const FRESH_DAYS: f32 = 45.0;
/// The share of trees and bushes that flower as they come into leaf, and
/// for how long
const BLOSSOMING: f32 = 0.3;
const BLOSSOM_DAYS: f32 = 25.0;
const TURN: (f32, f32) = (250.0, 290.0);
const TURN_DAYS: f32 = 7.0;
const DROP_AFTER: (f32, f32) = (25.0, 45.0);

/// What the season does to a bundle's terrain, worked out once per mesh
pub(crate) struct Seasonal {
    leaf_day: Option<u16>,
    seed: u64,
    /// Metres above the slab's lowest ground
    heights: Vec<f32>,
    snowy: Vec<bool>,
    frozen: Vec<bool>,
}

/// How far through the year one plant is
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Leaves {
    Bare,
    /// The share of the way from the summer colour to `.0`
    Tinted([u8; 4], f32),
}

impl Leaves {
    pub(super) fn on(self, summer: [u8; 4], bare: [u8; 4]) -> [u8; 4] {
        match self {
            Leaves::Bare => bare,
            Leaves::Tinted(_, 0.0) => summer,
            Leaves::Tinted(colour, share) => blend(summer, colour, share),
        }
    }
}

impl Seasonal {
    pub(crate) fn new(bundle: &SceneBundle) -> Seasonal {
        let (grid, terrain, season) = (&bundle.grid, &bundle.terrain, &bundle.season);
        let cells = grid.cells();
        let metres: Vec<f32> = (0..cells).map(|cell| terrain.height_m(cell)).collect();
        let lowest = metres.iter().copied().fold(f32::INFINITY, f32::min);
        let seed = bundle.prop_seed;
        let mut frozen = vec![false; cells];
        let mut levels = vec![None; cells];
        for body in &bundle.water {
            let iced = season.ice_line_m.is_some_and(|line| body.level_m >= line);
            for &cell in &body.cells {
                if let Some(at) = levels.get_mut(cell as usize) {
                    *at = Some(body.level_m);
                    frozen[cell as usize] = iced;
                }
            }
        }
        let water = u8::from(Landcover::Water);
        let (cols, rows) = (grid.cols as usize, grid.rows as usize);
        let cell_m = grid.cell_size_m as f32;
        // Central differences, one-sided at the slab's edges
        let steep = |cell: usize| {
            let (col, row) = (cell % cols, cell / cols);
            let at = |c: usize, r: usize| metres[r * cols + c];
            let rise = |a: usize, b: usize, height: &dyn Fn(usize) -> f32| {
                (height(b) - height(a)) / ((b - a).max(1) as f32 * cell_m)
            };
            let (left, right) = (col.saturating_sub(1), (col + 1).min(cols - 1));
            let (up, down) = (row.saturating_sub(1), (row + 1).min(rows - 1));
            let across = rise(left, right, &|c| at(c, row));
            let along = rise(up, down, &|r| at(col, r));
            across.hypot(along).atan().to_degrees() > SNOW_MAX_SLOPE_DEG
        };
        let snowy = match season.snow_line_m {
            None => vec![false; cells],
            Some(line) => (0..cells)
                .map(|cell| {
                    let reach = unit(hash(seed, [SNOW_KEY, cell as u64])).powi(2) * PATCHY_M;
                    let open_water = terrain.landcover[cell] == water && !frozen[cell];
                    let ground = levels[cell].unwrap_or(metres[cell]);
                    !open_water && ground >= line - reach && (frozen[cell] || !steep(cell))
                })
                .collect(),
        };
        Seasonal {
            leaf_day: season.leaf_day,
            seed,
            heights: metres.iter().map(|m| m - lowest).collect(),
            snowy,
            frozen,
        }
    }

    pub(super) fn snowy(&self, cell: usize) -> bool {
        self.snowy.get(cell).copied().unwrap_or(false)
    }

    pub(super) fn frozen(&self, cell: usize) -> bool {
        self.frozen.get(cell).copied().unwrap_or(false)
    }

    /// The year's day for plants at this cell's height
    fn day(&self, cell: usize) -> Option<f32> {
        let day = f32::from(self.leaf_day?);
        let shift = self.heights.get(cell).copied().unwrap_or(0.0) * DAYS_PER_M;
        Some(if day <= 182.0 {
            day - shift
        } else {
            day + shift
        })
    }

    /// One deciduous plant's leaves, from three rolls in [0, 1)
    pub(super) fn leaves(&self, cell: usize, rolls: [f32; 3]) -> Leaves {
        let Some(day) = self.day(cell) else {
            return Leaves::Tinted(FRESH, 0.0);
        };
        let span = |(from, to): (f32, f32), roll: f32| from + (to - from) * roll;
        let out = span(LEAF_OUT, rolls[0]);
        let turn = span(TURN, rolls[1]);
        let drop = turn + span(DROP_AFTER, rolls[2]);
        if day < out || day >= drop {
            Leaves::Bare
        } else if rolls[1] < BLOSSOMING && day < out + BLOSSOM_DAYS {
            Leaves::Tinted(BLOSSOM[usize::from(rolls[0] >= 0.5)], 1.0)
        } else if day >= turn {
            let colour = AUTUMN[(rolls[2] * AUTUMN.len() as f32) as usize % AUTUMN.len()];
            Leaves::Tinted(colour, ((day - turn) / TURN_DAYS).min(1.0))
        } else {
            Leaves::Tinted(FRESH, 1.0 - ((day - out) / FRESH_DAYS).clamp(0.0, 1.0))
        }
    }

    /// Each cell's ground: its landcover in the season's colours, under
    /// snow, or frozen over
    pub(super) fn ground(&self, bundle: &SceneBundle) -> Vec<[u8; 4]> {
        bundle
            .terrain
            .landcover
            .iter()
            .enumerate()
            .map(|(cell, &class)| self.cell(cell, class))
            .collect()
    }

    /// How much of spring this cell's plants show, for fresh growth on
    /// conifers
    pub(super) fn spring(&self, cell: usize) -> f32 {
        self.day(cell).map_or(0.0, |day| Weights::on(day).spring)
    }

    fn cell(&self, cell: usize, class: u8) -> [u8; 4] {
        let summer = landcover_rgba(class);
        if self.snowy(cell) {
            return SNOW;
        }
        if self.frozen(cell) {
            return ICE;
        }
        let Some(day) = self.day(cell) else {
            return summer;
        };
        let roll = |k: u64| unit(hash(self.seed, [CELL_KEY, cell as u64, k]));
        let rolls = [1, 2, 3].map(roll);
        let weights = Weights::on(day);
        let litter = |strength: f32| {
            let fallen = self.leaves(cell, rolls).on(summer, LITTER);
            blend(summer, fallen, strength)
        };
        let Ok(class) = Landcover::try_from(class) else {
            return summer;
        };
        let ground = match class {
            Landcover::Broadleaf => litter(0.6),
            Landcover::Scrub => litter(0.5),
            Landcover::MixedForest => litter(0.35),
            _ => {
                let (least, most) = SPREAD;
                let spread = least + (most - least) * rolls[0];
                tints(class)
                    .into_iter()
                    .zip([weights.spring, weights.autumn, weights.winter])
                    .fold(summer, |colour, ((target, strength), weight)| {
                        blend(colour, target, (strength * weight * spread).min(1.0))
                    })
            }
        };
        let flowering = class == Landcover::Grassland && weights.spring >= 0.5;
        let speck = hash(self.seed, [CELL_KEY, cell as u64, 4]);
        if flowering && speck % 1000 < FLOWER_PER_MILLE {
            FLOWERS[(speck / 1000 % FLOWERS.len() as u64) as usize]
        } else {
            ground
        }
    }

    /// The ground colour of a frozen lake cell away from its shore
    pub(super) fn ice(&self, cell: usize) -> [u8; 4] {
        if self.snowy(cell) {
            SNOW
        } else if hash(self.seed, [CELL_KEY, cell as u64, 0]) % 1000 < CRACK_PER_MILLE {
            CRACK
        } else {
            ICE
        }
    }
}

/// How the trees on the slab's lowest ground look on a day of the year
/// counted as in the north
pub fn foliage(leaf_day: u16) -> &'static str {
    let weights = Weights::on(f32::from(leaf_day));
    if weights.winter >= 0.5 {
        "bare trees"
    } else if weights.spring >= 0.5 {
        "spring leaves"
    } else if weights.autumn >= 0.5 {
        "autumn colours"
    } else {
        "summer leaves"
    }
}

/// How much of each season, from 0 to 1, shows on open ground on a day of
/// the year counted as in the north; summer is what's left
#[derive(Clone, Copy, Debug, PartialEq)]
struct Weights {
    spring: f32,
    autumn: f32,
    winter: f32,
}

impl Weights {
    fn on(day: f32) -> Weights {
        let rise = |from: f32, to: f32| ((day - from) / (to - from)).clamp(0.0, 1.0);
        Weights {
            spring: rise(70.0, 100.0) * (1.0 - rise(140.0, 170.0)),
            autumn: rise(245.0, 275.0) * (1.0 - rise(310.0, 335.0)),
            winter: rise(310.0, 335.0).max(1.0 - rise(60.0, 90.0)),
        }
    }
}

/// What open ground turns towards in spring, autumn and winter, and how far
fn tints(class: Landcover) -> [([u8; 4], f32); 3] {
    match class {
        Landcover::Grassland => [(FRESH, 0.5), (GOLDEN, 0.62), (DULL, 0.65)],
        Landcover::Farmland => [(YOUNG_CROP, 0.6), (PLOUGHED, 0.55), (DULL, 0.6)],
        Landcover::Heath => [(FRESH, 0.0), (BRACKEN, 0.5), (DARK_HEATH, 0.3)],
        Landcover::Wetland => [(FRESH, 0.2), (GOLDEN, 0.3), (DULL, 0.4)],
        _ => [(FRESH, 0.0); 3],
    }
}

pub(super) fn unit(hash: u64) -> f32 {
    (hash >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::{Season, WaterBody};
    use isotrail_testkit::{bundle, grid};

    use super::*;

    fn slope(classes: &[Landcover], season: Season) -> (SceneBundle, Seasonal) {
        let heights: Vec<f32> = (0..classes.len()).map(|i| i as f32 * 10.0).collect();
        let mut bundle = bundle(grid(classes.len() as u32, 1, 100.0), &heights, classes);
        bundle.season = season;
        let seasonal = Seasonal::new(&bundle);
        (bundle, seasonal)
    }

    fn day(day: u16) -> Season {
        Season {
            leaf_day: Some(day),
            ..Season::default()
        }
    }

    #[test]
    fn a_neutral_season_keeps_the_landcover_colours() {
        let classes = Landcover::ALL.to_vec();
        let (bundle, seasonal) = slope(&classes, Season::default());
        let summer: Vec<[u8; 4]> = classes.iter().map(|&c| landcover_rgba(c.into())).collect();
        assert_eq!(seasonal.ground(&bundle), summer);
        assert_eq!(seasonal.leaves(0, [0.5; 3]).on(FRESH, LITTER), FRESH);
    }

    #[test]
    fn foliage_names_the_time_of_year() {
        let names = [20, 125, 200, 290, 340].map(foliage);
        assert_eq!(
            names,
            [
                "bare trees",
                "spring leaves",
                "summer leaves",
                "autumn colours",
                "bare trees"
            ]
        );
    }

    #[test]
    fn trees_leaf_out_turn_and_drop_through_the_year() {
        let (_, winter) = slope(&[Landcover::Broadleaf], day(20));
        assert_eq!(winter.leaves(0, [0.5; 3]), Leaves::Bare);
        let (_, spring) = slope(&[Landcover::Broadleaf], day(125));
        assert!(matches!(spring.leaves(0, [0.2, 0.5, 0.2]), Leaves::Tinted(FRESH, s) if s > 0.0));
        assert_eq!(
            spring.leaves(0, [0.2, 0.1, 0.2]),
            Leaves::Tinted(BLOSSOM[0], 1.0)
        );
        assert_eq!(spring.leaves(0, [0.9; 3]), Leaves::Bare);
        let (_, summer) = slope(&[Landcover::Broadleaf], day(200));
        assert_eq!(summer.leaves(0, [0.5; 3]), Leaves::Tinted(FRESH, 0.0));
        let (_, autumn) = slope(&[Landcover::Broadleaf], day(290));
        let turned = (0..20)
            .map(|i| autumn.leaves(0, [0.5, i as f32 / 20.0, 0.3]))
            .filter(|l| matches!(l, Leaves::Tinted(c, s) if AUTUMN.contains(c) && *s > 0.0))
            .count();
        assert!((5..20).contains(&turned), "{turned}");
    }

    #[test]
    fn higher_ground_runs_behind_in_spring_and_ahead_in_autumn() {
        let at = |heights: [f32; 2], leaf_day: u16| {
            let mut bundle = bundle(grid(2, 1, 100.0), &heights, &[Landcover::Broadleaf; 2]);
            bundle.season = day(leaf_day);
            let seasonal = Seasonal::new(&bundle);
            [0, 1].map(|cell| seasonal.leaves(cell, [0.5; 3]))
        };
        assert!(matches!(
            at([0.0, 1500.0], 130),
            [Leaves::Tinted(..), Leaves::Bare]
        ));
        assert!(matches!(
            at([0.0, 1500.0], 285),
            [Leaves::Tinted(..), Leaves::Bare]
        ));
        assert!(matches!(
            at([0.0, 0.0], 130),
            [Leaves::Tinted(..), Leaves::Tinted(..)]
        ));
    }

    #[test]
    fn open_ground_shows_every_season() {
        assert_eq!(
            Weights::on(125.0),
            Weights {
                spring: 1.0,
                autumn: 0.0,
                winter: 0.0
            }
        );
        assert_eq!(
            Weights::on(200.0),
            Weights {
                spring: 0.0,
                autumn: 0.0,
                winter: 0.0
            }
        );
        assert_eq!(
            Weights::on(290.0),
            Weights {
                spring: 0.0,
                autumn: 1.0,
                winter: 0.0
            }
        );
        assert_eq!(
            Weights::on(15.0),
            Weights {
                spring: 0.0,
                autumn: 0.0,
                winter: 1.0
            }
        );
        let classes = [
            Landcover::Heath,
            Landcover::Grassland,
            Landcover::Farmland,
            Landcover::Rock,
        ];
        let summer: Vec<[u8; 4]> = classes.iter().map(|&c| landcover_rgba(c.into())).collect();
        for leaf_day in [125, 290, 15] {
            let (bundle, seasonal) = slope(&classes, day(leaf_day));
            let ground = seasonal.ground(&bundle);
            assert_ne!(ground[1..3], summer[1..3], "{leaf_day}");
            assert_eq!(ground[3], summer[3], "{leaf_day}");
            assert_eq!(ground[0] == summer[0], leaf_day == 125, "{leaf_day}");
        }
        let (bundle, seasonal) = slope(&classes, day(200));
        assert_eq!(seasonal.ground(&bundle), summer);
    }

    #[test]
    fn spring_meadows_flower() {
        let meadow = [Landcover::Grassland; 400];
        let flowers = |leaf_day| {
            let (bundle, seasonal) = slope(&meadow, day(leaf_day));
            let ground = seasonal.ground(&bundle);
            ground.iter().filter(|c| FLOWERS.contains(c)).count()
        };
        assert!((10..50).contains(&flowers(125)), "{}", flowers(125));
        assert_eq!(flowers(200), 0);
    }

    #[test]
    fn snow_lies_above_the_line_in_patches_below_it_but_not_on_cliffs_or_open_water() {
        let classes = [Landcover::Grassland; 40];
        let season = Season {
            snow_line_m: Some(200.0),
            ..Season::default()
        };
        let (gentle, seasonal) = slope(&classes, season);
        let ground = seasonal.ground(&gentle);
        assert!(ground[..5].iter().all(|&c| c != SNOW));
        assert!(ground[20..].iter().all(|&c| c == SNOW));
        let patches = ground[5..20].iter().filter(|&&c| c == SNOW).count();
        assert!((1..15).contains(&patches), "{patches}");

        let heights: Vec<f32> = (0..40).map(|i| 300.0 + i as f32 * 150.0).collect();
        let mut cliff = bundle(grid(40, 1, 100.0), &heights, &classes);
        cliff.season = season;
        assert!(Seasonal::new(&cliff).snowy.iter().all(|s| !s));

        let mut lake = bundle(grid(3, 1, 100.0), &[500.0; 3], &[Landcover::Water; 3]);
        lake.season = season;
        assert!(Seasonal::new(&lake).snowy.iter().all(|s| !s));
        lake.season.ice_line_m = Some(400.0);
        assert!(Seasonal::new(&lake).snowy.iter().all(|&s| s));
    }

    #[test]
    fn lakes_freeze_above_the_ice_line() {
        let mut classes = vec![Landcover::Grassland; 6];
        classes[1] = Landcover::Water;
        classes[4] = Landcover::Water;
        let heights = [0.0, 100.0, 0.0, 0.0, 300.0, 0.0];
        let mut bundle = bundle(grid(6, 1, 100.0), &heights, &classes);
        bundle.water = vec![
            WaterBody {
                level_m: 100.0,
                cells: vec![1],
            },
            WaterBody {
                level_m: 300.0,
                cells: vec![4],
            },
        ];
        bundle.season.ice_line_m = Some(200.0);
        let seasonal = Seasonal::new(&bundle);
        assert_eq!(seasonal.frozen, [false, false, false, false, true, false]);
        assert!([ICE, CRACK].contains(&seasonal.ice(4)));
        let cracked = (0..2000)
            .filter(|&seed| {
                bundle.prop_seed = seed;
                Seasonal::new(&bundle).ice(4) == CRACK
            })
            .count();
        assert!((60..200).contains(&cracked), "{cracked}");
    }
}
