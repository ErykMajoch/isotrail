mod raster;
mod tags;
mod terrain;
mod water;

use std::collections::BTreeMap;

use isotrail_bundle::{Landcover, line_mask};
use tracing::{debug, info};

pub use terrain::{
    ALPINE_ROCK_SLOPE_DEG, ROCK_ABOVE_TREELINE_M, ROCK_SLOPE_DEG, Treeline, latitude_treeline_m,
    slope_deg,
};
pub use water::{WATER_LEVEL_PERCENTILE, WaterBody};

use self::tags::{LineEffect, area_class, is_lake, line_effect, priority};
use self::water::{NO_LAKE, level_lakes};
use crate::{FEATURE_KEYS, Grid, Heightmap, OsmFeatures};

/// What every grid cell is, row-major like [`Heightmap`]
#[derive(Debug, Clone, PartialEq)]
pub struct LandcoverMap {
    pub cols: u32,
    pub rows: u32,
    pub classes: Vec<Landcover>,
    /// [`line_mask`] bits per cell
    pub lines: Vec<u8>,
    pub water: Vec<WaterBody>,
    pub treeline: Treeline,
}

impl LandcoverMap {
    /// Rasterises the map features onto the grid, fills the cells they leave
    /// open from the terrain, and flattens lakes in `heightmap` to their
    /// water level
    pub fn classify(
        grid: &Grid,
        features: &OsmFeatures,
        heightmap: &mut Heightmap,
    ) -> LandcoverMap {
        let (cols, rows) = (grid.cols, grid.rows);
        assert_eq!(
            (heightmap.cols, heightmap.rows),
            (cols, rows),
            "the heightmap belongs to another grid"
        );
        let cells = (cols * rows) as usize;
        let to_cell = |&(lat, lon): &(f64, f64)| {
            let (x, y) = grid.footprint.projection.project(lat, lon);
            grid.local_to_cell(x, y)
        };

        let mut claimed: Vec<Option<Landcover>> = vec![None; cells];
        let mut lakes = vec![NO_LAKE; cells];
        let claim = |claimed: &mut [Option<Landcover>], i: usize, class: Landcover| {
            if claimed[i].is_none_or(|c| priority(class) > priority(c)) {
                claimed[i] = Some(class);
            }
        };
        let mut unknown: BTreeMap<String, usize> = BTreeMap::new();

        for (lake, area) in features.areas.iter().enumerate() {
            let Some(class) = area_class(&area.tags) else {
                if let Some(tag) = feature_tag(&area.tags) {
                    *unknown.entry(tag).or_default() += 1;
                }
                continue;
            };
            let lake = (class == Landcover::Water && is_lake(&area.tags)).then_some(lake as u32);
            for polygon in &area.polygons {
                let rings: Vec<Vec<(f64, f64)>> = std::iter::once(&polygon.outer)
                    .chain(&polygon.inner)
                    .map(|ring| ring.iter().map(to_cell).collect())
                    .collect();
                raster::fill_polygon(&rings, cols, rows, |col, row| {
                    let i = (row * cols + col) as usize;
                    claim(&mut claimed, i, class);
                    if let Some(lake) = lake
                        && lakes[i] == NO_LAKE
                    {
                        lakes[i] = lake;
                    }
                });
            }
        }

        let mut lines = vec![0u8; cells];
        for line in &features.lines {
            let Some(effect) = line_effect(&line.tags) else {
                continue;
            };
            let points: Vec<(f64, f64)> = line.points.iter().map(to_cell).collect();
            raster::trace_line(&points, cols, rows, |col, row| {
                let i = (row * cols + col) as usize;
                match effect {
                    LineEffect::Cover(class) => claim(&mut claimed, i, class),
                    LineEffect::Mask(bit) => lines[i] |= bit,
                }
            });
        }
        if !unknown.is_empty() {
            debug!(?unknown, "map features with no landcover class");
        }

        let mut forest_heights: Vec<f32> = claimed
            .iter()
            .zip(&heightmap.heights_m)
            .filter(|(class, _)| class.is_some_and(Landcover::is_forest))
            .map(|(_, &h)| h)
            .collect();
        let lat = grid.footprint.projection.origin().0;
        let treeline = Treeline::estimate(&mut forest_heights, cells, lat);
        info!(?treeline, "treeline estimate in metres");

        let classes = claimed
            .iter()
            .enumerate()
            .map(|(i, class)| {
                class.unwrap_or_else(|| {
                    let (col, row) = (i as u32 % cols, i as u32 / cols);
                    let slope = slope_deg(heightmap, grid.cell_size_m, col, row);
                    terrain::fallback_class(slope, f64::from(heightmap.heights_m[i]), treeline)
                })
            })
            .collect();

        let water = level_lakes(&lakes, heightmap);
        for body in &water {
            debug!(
                level_m = body.level_m,
                cells = body.cells.len(),
                "water body"
            );
        }

        LandcoverMap {
            cols,
            rows,
            classes,
            lines,
            water,
            treeline,
        }
    }

    /// Classes by the fraction of cells they cover, largest first, leaving
    /// out classes with no cells
    pub fn shares(&self) -> Vec<(Landcover, f64)> {
        let mut counts = [0usize; Landcover::ALL.len()];
        for &class in &self.classes {
            counts[usize::from(u8::from(class))] += 1;
        }
        let total = self.classes.len().max(1) as f64;
        let mut shares: Vec<_> = Landcover::ALL
            .into_iter()
            .zip(counts)
            .filter(|&(_, n)| n > 0)
            .map(|(class, n)| (class, n as f64 / total))
            .collect();
        shares.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        shares
    }

    /// Cells with each [`line_mask`] bit set
    pub fn line_counts(&self) -> [(u8, usize); 3] {
        [line_mask::PATH, line_mask::ROAD, line_mask::STREAM]
            .map(|bit| (bit, self.lines.iter().filter(|&&l| l & bit != 0).count()))
    }

    /// Each cell in its [`Landcover::legend_rgb`] colour
    pub fn legend_image(&self) -> Vec<u8> {
        self.classes.iter().flat_map(|&c| c.legend_rgb()).collect()
    }

    /// Levelled water white, other water (rivers) grey, land black
    pub fn water_mask(&self) -> Vec<u8> {
        let mut mask: Vec<u8> = self
            .classes
            .iter()
            .map(|&c| if c == Landcover::Water { 128 } else { 0 })
            .collect();
        for &cell in self.water.iter().flat_map(|b| &b.cells) {
            mask[cell as usize] = 255;
        }
        mask
    }

    /// Paths white, roads orange and streams blue on black, in that order of
    /// precedence
    pub fn line_image(&self) -> Vec<u8> {
        self.lines
            .iter()
            .flat_map(|&bits| {
                if bits & line_mask::PATH != 0 {
                    [255, 255, 255]
                } else if bits & line_mask::ROAD != 0 {
                    [255, 160, 32]
                } else if bits & line_mask::STREAM != 0 {
                    [64, 140, 255]
                } else {
                    [0, 0, 0]
                }
            })
            .collect()
    }
}

fn feature_tag(tags: &crate::Tags) -> Option<String> {
    FEATURE_KEYS
        .iter()
        .find_map(|&key| Some(format!("{key}={}", tags.get(key)?)))
}

/// Nearest-rank percentile, `p` in [0, 1]; sorts `values`
fn percentile(values: &mut [f32], p: f64) -> f32 {
    values.sort_by(f32::total_cmp);
    let rank = ((values.len() - 1) as f64 * p).round() as usize;
    values[rank]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::num::NonZeroU32;

    use isotrail_track::{Segment, Track, TrackPoint};

    use super::*;
    use crate::{Area, Footprint, Line, LocalProjection, OsmId, OsmKind, Polygon, Ring, Tags};

    /// North-up, 52 × 26 cells of 200 m
    fn grid() -> Grid {
        let projection = LocalProjection::new(46.5, 10.0);
        let points = [(0.0, 0.0), (8_000.0, 0.0)]
            .map(|(x, y)| {
                let (lat, lon) = projection.unproject(x, y);
                TrackPoint {
                    lat,
                    lon,
                    ele: None,
                    time: None,
                }
            })
            .to_vec();
        let track = Track {
            segments: vec![Segment { points }],
            ..Track::default()
        };
        let grid = Grid::new(
            &Footprint::from_track(&track).unwrap(),
            NonZeroU32::new(52).unwrap(),
        );
        assert_eq!((grid.cols, grid.rows), (52, 26));
        grid
    }

    /// (lat, lon) of a fractional cell position
    fn at(grid: &Grid, col: f64, row: f64) -> (f64, f64) {
        let u = col * grid.cell_size_m - grid.footprint.length_m / 2.0;
        let v = grid.footprint.width_m / 2.0 - row * grid.cell_size_m;
        let (x, y) = grid.footprint.to_local(u, v);
        grid.footprint.projection.unproject(x, y)
    }

    fn rect(grid: &Grid, c0: f64, r0: f64, c1: f64, r1: f64) -> Ring {
        [(c0, r0), (c1, r0), (c1, r1), (c0, r1), (c0, r0)]
            .map(|(c, r)| at(grid, c, r))
            .to_vec()
    }

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|&(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    fn id(id: i64) -> OsmId {
        OsmId {
            kind: OsmKind::Way,
            id,
        }
    }

    fn area(pairs: &[(&str, &str)], outer: Ring, inner: Vec<Ring>) -> Area {
        Area {
            id: id(1),
            tags: tags(pairs),
            polygons: vec![Polygon { outer, inner }],
        }
    }

    fn line(grid: &Grid, pairs: &[(&str, &str)], cells: &[(f64, f64)]) -> Line {
        Line {
            id: id(2),
            tags: tags(pairs),
            points: cells.iter().map(|&(c, r)| at(grid, c, r)).collect(),
        }
    }

    fn flat(grid: &Grid, height_m: f32) -> Heightmap {
        Heightmap {
            cols: grid.cols,
            rows: grid.rows,
            heights_m: vec![height_m; (grid.cols * grid.rows) as usize],
        }
    }

    fn cells_of(map: &LandcoverMap, class: Landcover) -> BTreeSet<(u32, u32)> {
        (0..map.classes.len() as u32)
            .filter(|&i| map.classes[i as usize] == class)
            .map(|i| (i % map.cols, i / map.cols))
            .collect()
    }

    fn block(cols: std::ops::Range<u32>, rows: std::ops::Range<u32>) -> BTreeSet<(u32, u32)> {
        rows.flat_map(|r| cols.clone().map(move |c| (c, r)))
            .collect()
    }

    #[test]
    fn water_beats_forest_in_either_order() {
        let grid = grid();
        let wood = area(
            &[("natural", "wood")],
            rect(&grid, 2.0, 2.0, 12.0, 12.0),
            vec![],
        );
        let lake = area(
            &[("natural", "water")],
            rect(&grid, 5.0, 5.0, 8.0, 8.0),
            vec![],
        );
        for areas in [vec![wood.clone(), lake.clone()], vec![lake, wood]] {
            let features = OsmFeatures {
                areas,
                lines: vec![],
            };
            let map = LandcoverMap::classify(&grid, &features, &mut flat(&grid, 500.0));
            assert_eq!(cells_of(&map, Landcover::Water), block(5..8, 5..8));
            let forest: BTreeSet<_> = block(2..12, 2..12)
                .difference(&block(5..8, 5..8))
                .copied()
                .collect();
            assert_eq!(cells_of(&map, Landcover::MixedForest), forest);
            assert_eq!(
                cells_of(&map, Landcover::Grassland).len(),
                52 * 26 - 100,
                "the rest is filled from the terrain"
            );
            assert_eq!(map.water.len(), 1);
        }
    }

    #[test]
    fn islands_stay_land_and_lakes_are_levelled() {
        let grid = grid();
        let lake = area(
            &[("natural", "water"), ("water", "lake")],
            rect(&grid, 10.0, 4.0, 20.0, 14.0),
            vec![rect(&grid, 13.0, 7.0, 16.0, 10.0)],
        );
        let mut heightmap = flat(&grid, 500.0);
        for (i, h) in heightmap.heights_m.iter_mut().enumerate() {
            *h += (i % 7) as f32;
        }
        let before = heightmap.clone();
        let features = OsmFeatures {
            areas: vec![lake],
            lines: vec![],
        };
        let map = LandcoverMap::classify(&grid, &features, &mut heightmap);

        let water: BTreeSet<_> = block(10..20, 4..14)
            .difference(&block(13..16, 7..10))
            .copied()
            .collect();
        assert_eq!(cells_of(&map, Landcover::Water), water);
        let [body] = &map.water[..] else {
            panic!("{:?}", map.water);
        };
        assert_eq!(body.cells.len(), water.len());
        assert!(body.level_m < 502.0, "{}", body.level_m);
        for (i, (&h, &was)) in heightmap
            .heights_m
            .iter()
            .zip(&before.heights_m)
            .enumerate()
        {
            let cell = (i as u32 % grid.cols, i as u32 / grid.cols);
            let expected = if water.contains(&cell) {
                body.level_m
            } else {
                was
            };
            assert_eq!(h, expected, "{cell:?}");
        }
        assert_eq!(
            map.water_mask().iter().filter(|&&m| m == 255).count(),
            water.len()
        );
    }

    #[test]
    fn rivers_are_continuous_water_but_not_levelled() {
        let grid = grid();
        let features = OsmFeatures {
            areas: vec![area(
                &[("natural", "water"), ("water", "river")],
                rect(&grid, 40.0, 0.0, 44.0, 26.0),
                vec![],
            )],
            lines: vec![line(
                &grid,
                &[("waterway", "river")],
                &[(-1.0, 3.2), (17.3, 9.9), (51.7, 20.9), (55.0, 22.0)],
            )],
        };
        let mut heightmap = flat(&grid, 300.0);
        let map = LandcoverMap::classify(&grid, &features, &mut heightmap);
        assert!(map.water.is_empty());
        assert!(heightmap.heights_m.iter().all(|&h| h == 300.0));
        assert!(map.water_mask().iter().all(|&m| m != 255));

        let water = cells_of(&map, Landcover::Water);
        let start = *water
            .iter()
            .find(|&&(c, _)| c == 0)
            .expect("reaches the west edge");
        let mut reached = BTreeSet::from([start]);
        let mut todo = vec![start];
        while let Some((c, r)) = todo.pop() {
            for n in [
                (c.wrapping_sub(1), r),
                (c + 1, r),
                (c, r.wrapping_sub(1)),
                (c, r + 1),
            ] {
                if water.contains(&n) && reached.insert(n) {
                    todo.push(n);
                }
            }
        }
        assert!(reached.iter().any(|&(c, _)| c == 51), "the river is broken");
    }

    #[test]
    fn ways_set_mask_bits_and_cliffs_are_rock() {
        let grid = grid();
        let features = OsmFeatures {
            areas: vec![],
            lines: vec![
                line(&grid, &[("highway", "path")], &[(0.5, 1.5), (9.5, 1.5)]),
                line(
                    &grid,
                    &[("highway", "secondary")],
                    &[(4.5, 0.5), (4.5, 5.5)],
                ),
                line(&grid, &[("waterway", "stream")], &[(0.5, 5.5), (9.5, 5.5)]),
                line(
                    &grid,
                    &[("natural", "cliff")],
                    &[(20.5, 10.5), (25.5, 10.5)],
                ),
                line(&grid, &[("waterway", "dam")], &[(30.5, 0.5), (30.5, 20.5)]),
            ],
        };
        let map = LandcoverMap::classify(&grid, &features, &mut flat(&grid, 500.0));
        let bits = |c: u32, r: u32| map.lines[(r * grid.cols + c) as usize];
        assert_eq!(bits(0, 1), line_mask::PATH);
        assert_eq!(bits(4, 1), line_mask::PATH | line_mask::ROAD);
        assert_eq!(bits(4, 5), line_mask::ROAD | line_mask::STREAM);
        assert_eq!(bits(9, 5), line_mask::STREAM);
        assert_eq!(bits(30, 3), 0);
        assert_eq!(
            map.line_counts(),
            [
                (line_mask::PATH, 10),
                (line_mask::ROAD, 6),
                (line_mask::STREAM, 10)
            ]
        );
        assert_eq!(cells_of(&map, Landcover::Rock), block(20..26, 10..11));
        assert_eq!(cells_of(&map, Landcover::Grassland).len(), 52 * 26 - 6);
    }

    #[test]
    fn steep_ground_above_the_forest_is_rock() {
        let grid = grid();
        let features = OsmFeatures {
            areas: vec![area(
                &[("landuse", "forest"), ("leaf_type", "needleleaved")],
                rect(&grid, 0.0, 0.0, 10.0, 26.0),
                vec![],
            )],
            lines: vec![],
        };
        let mut heightmap = flat(&grid, 0.0);
        for (i, h) in heightmap.heights_m.iter_mut().enumerate() {
            *h = (i as u32 % grid.cols) as f32 * 150.0;
        }
        let map = LandcoverMap::classify(&grid, &features, &mut heightmap);
        assert_eq!(map.treeline, Treeline::Forest(1350.0));
        assert_eq!(cells_of(&map, Landcover::Coniferous), block(0..10, 0..26));
        assert_eq!(cells_of(&map, Landcover::Rock), block(10..52, 0..26));
    }

    #[test]
    fn summaries_describe_the_map() {
        let grid = grid();
        let features = OsmFeatures {
            areas: vec![area(
                &[("natural", "scree")],
                rect(&grid, 0.0, 0.0, 13.0, 26.0),
                vec![],
            )],
            lines: vec![],
        };
        let map = LandcoverMap::classify(&grid, &features, &mut flat(&grid, 500.0));
        assert_eq!(
            map.shares(),
            [(Landcover::Grassland, 0.75), (Landcover::Scree, 0.25)]
        );
        let image = map.legend_image();
        assert_eq!(image.len(), 52 * 26 * 3);
        assert_eq!(image[..3], Landcover::Scree.legend_rgb());
        assert_eq!(image[image.len() - 3..], Landcover::Grassland.legend_rgb());
        assert_eq!(map.line_image().len(), 52 * 26 * 3);
        assert_eq!(map.water_mask(), vec![0; 52 * 26]);
        assert!(matches!(map.treeline, Treeline::Latitude(_)));
    }

    #[test]
    fn percentile_is_nearest_rank() {
        let mut values = [5.0, 1.0, 4.0, 2.0, 3.0];
        assert_eq!(percentile(&mut values, 0.0), 1.0);
        assert_eq!(percentile(&mut values, 0.5), 3.0);
        assert_eq!(percentile(&mut values, 1.0), 5.0);
        assert_eq!(percentile(&mut [7.0], 0.1), 7.0);
    }
}
