use isotrail_bundle::{
    Bounds, GridInfo, HikeStats, Landcover, Metadata, ProfileSample, Provenance, Route, RoutePoint,
    SceneBundle, Season, Terrain, WaterBody, Weather, WeatherDataset, WeatherSample, line_mask,
};

/// An unrotated grid centred on the origin
pub fn grid(cols: u32, rows: u32, cell_size_m: f64) -> GridInfo {
    GridInfo {
        cols,
        rows,
        cell_size_m,
        origin_lat: 0.0,
        origin_lon: 0.0,
        center_m: (0.0, 0.0),
        rotation_rad: 0.0,
        exaggeration: 1.0,
    }
}

/// A bundle with no route; water cells become one lake at their lowest height
///
/// # Panics
///
/// If the layers don't have one value per cell
pub fn bundle(grid: GridInfo, heights_m: &[f32], landcover: &[Landcover]) -> SceneBundle {
    assert_eq!(heights_m.len(), grid.cells(), "one height per cell");
    assert_eq!(landcover.len(), grid.cells(), "one class per cell");
    let mut terrain = Terrain::quantise(heights_m);
    terrain.landcover = landcover.iter().map(|&c| u8::from(c)).collect();
    terrain.lines = vec![0; grid.cells()];
    let lake: Vec<u32> = (0..grid.cells() as u32)
        .filter(|&i| landcover[i as usize] == Landcover::Water)
        .collect();
    let water = if lake.is_empty() {
        Vec::new()
    } else {
        let level_m = lake
            .iter()
            .map(|&i| terrain.height_m(i as usize))
            .fold(f32::INFINITY, f32::min);
        vec![WaterBody {
            level_m,
            cells: lake,
            sea: false,
        }]
    };
    let mut bundle = SceneBundle {
        meta: Metadata {
            title: "Synthetic hill".into(),
            name: None,
            desc: None,
            timezone: None,
            local_date: None,
            stats: HikeStats {
                points: 0,
                segments: 0,
                distance_m: 0.0,
                gain_m: None,
                loss_m: None,
                min_ele_m: None,
                max_ele_m: None,
                bounds: Bounds {
                    min_lat: 0.0,
                    min_lon: 0.0,
                    max_lat: 0.0,
                    max_lon: 0.0,
                },
                start: None,
                end: None,
                elapsed_s: None,
                moving_s: None,
            },
            attribution: Vec::new(),
            provenance: Provenance {
                generator: "isotrail-testkit".into(),
                elevation: "synthetic".into(),
                map_data: "synthetic".into(),
                weather: None,
            },
        },
        grid,
        terrain,
        water,
        prop_seed: 0,
        route: Route::default(),
        profile: Vec::new(),
        sun: Vec::new(),
        moon: Vec::new(),
        weather: None,
        season: Season::default(),
    };
    bundle.prop_seed = bundle.content_seed();
    bundle
}

/// A turned 96 x 48 slab with a tilted plain, a rocky hill ringed by scree, a
/// levelled lake fed by a stream, two forests, a path and a road, and a
/// straight route from the lake shore to the summit
pub fn diorama_bundle() -> SceneBundle {
    let grid = GridInfo {
        rotation_rad: 0.4,
        exaggeration: 1.5,
        ..grid(96, 48, 25.0)
    };
    let (cols, rows) = (grid.cols as usize, grid.rows as usize);
    let height = |c: f32, r: f32| {
        let hill = 260.0 * (-((c - 64.0).powi(2) + (r - 20.0).powi(2)) / 220.0).exp();
        let bowl = 70.0 * (-((c - 24.0).powi(2) + (r - 30.0).powi(2)) / 60.0).exp();
        let ridge = 25.0 * (c / 7.0).sin() * (r / 11.0).cos();
        (300.0 + 1.2 * c + hill + ridge - bowl, hill)
    };
    // The lake fills the bowl up to a few metres above its floor
    let level = (0..cols * rows)
        .map(|i| height((i % cols) as f32 + 0.5, (i / cols) as f32 + 0.5).0)
        .fold(f32::INFINITY, f32::min)
        + 30.0;
    let mut heights = Vec::with_capacity(grid.cells());
    let mut classes = Vec::with_capacity(grid.cells());
    let mut lines = Vec::with_capacity(grid.cells());
    for row in 0..rows {
        for col in 0..cols {
            let (c, r) = (col as f32 + 0.5, row as f32 + 0.5);
            let (h, hill) = height(c, r);
            let class = if h < level && (c - 24.0).abs() < 14.0 {
                Landcover::Water
            } else if hill > 190.0 {
                Landcover::Rock
            } else if hill > 130.0 {
                Landcover::Scree
            } else if (c - 58.0).powi(2) / 2.0 + (r - 38.0).powi(2) < 60.0 {
                Landcover::Coniferous
            } else if (c - 12.0).powi(2) + (r - 10.0).powi(2) < 50.0 {
                Landcover::Broadleaf
            } else {
                Landcover::Grassland
            };
            // A stream into the lake, a path through the conifers, a road
            let line = match (col, row) {
                _ if class == Landcover::Water => 0,
                (90, _) => line_mask::ROAD,
                (44.., 38) => line_mask::PATH,
                (..24, 30) => line_mask::STREAM,
                _ => 0,
            };
            heights.push(if class == Landcover::Water { level } else { h });
            classes.push(class);
            lines.push(line);
        }
    }
    let mut bundle = bundle(grid, &heights, &classes);
    bundle.terrain.lines = lines;
    let (from, to) = ((34.0f32, 40.0f32), (64.0f32, 20.0f32));
    let length_m = (to.0 - from.0).hypot(to.1 - from.1) * grid.cell_size_m as f32;
    bundle.route.points = (0..=10)
        .map(|i| {
            let t = i as f32 / 10.0;
            let (col, row) = (from.0 + t * (to.0 - from.0), from.1 + t * (to.1 - from.1));
            RoutePoint {
                col,
                row,
                distance_m: t * length_m,
                time: None,
                gps_ele_m: None,
                draped_m: heights[row as usize * cols + col as usize],
            }
        })
        .collect();
    bundle.prop_seed = bundle.content_seed();
    bundle
}

/// [`diorama_bundle`] with a title, a date, stats, an elevation profile and
/// data credits, keeping its prop seed so the terrain looks the same
pub fn hike_bundle() -> SceneBundle {
    let mut bundle = diorama_bundle();
    let meta = &mut bundle.meta;
    meta.title = "Zażółć gęślą jaźń | Łańcuch".into();
    meta.local_date = Some("2025-06-14".into());
    meta.attribution = vec!["© Synthetic contributors".into(), "Made-up Tiles".into()];
    let stats = &mut meta.stats;
    (stats.distance_m, stats.gain_m) = (7_420.0, Some(612.0));
    (stats.moving_s, stats.max_ele_m) = (Some(9_000), Some(842.0));
    let total = 7_420.0;
    bundle.profile = (0..64)
        .map(|i| {
            let t = i as f32 / 63.0;
            ProfileSample {
                distance_m: t * total,
                ele_m: 330.0 + 480.0 * t * t + 30.0 * (t * 19.0).sin(),
            }
        })
        .collect();
    bundle
}

/// Made-up hourly weather from `first` (Unix seconds on the hour): a cold
/// front clearing to sun, with rain then snow showers on the way
pub fn synthetic_weather(first: i64, hours: usize) -> Weather {
    let sample = |i: usize| {
        let h = i as f32;
        let (code, rain, snow) = match i % 6 {
            1 => (61, 0.6, 0.0),
            2 => (85, 0.0, 0.35),
            3 => (3, 0.0, 0.0),
            4 => (1, 0.0, 0.0),
            _ => (2, 0.0, 0.0),
        };
        WeatherSample {
            time: first + i as i64 * 3600,
            temperature_c: 2.0 + h * 1.5,
            weather_code: code,
            cloud_cover_pct: (90.0 - h * 15.0).max(10.0),
            cloud_low_pct: (70.0 - h * 15.0).max(0.0),
            cloud_mid_pct: 40.0,
            cloud_high_pct: 20.0,
            precipitation_mm: rain + snow / 0.7,
            rain_mm: rain,
            snowfall_cm: snow,
            snow_depth_m: 0.05,
            wind_speed_m_s: (8.0 - h).max(0.5),
            wind_direction_deg: (250.0 + h * 10.0) % 360.0,
        }
    };
    Weather {
        dataset: WeatherDataset::Archive,
        samples: (0..hours).map(sample).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundle_round_trips() {
        let mut weathered = hike_bundle();
        weathered.weather = Some(synthetic_weather(1_750_000_000 / 3600 * 3600, 12));
        let hill = bundle(
            grid(4, 3, 10.0),
            &[5.0, 7.0, 9.0, 4.0, 6.0, 8.0, 3.0, 5.0, 7.0, 2.0, 4.0, 6.0],
            &[
                Landcover::Water,
                Landcover::Water,
                Landcover::Grassland,
                Landcover::Rock,
            ]
            .repeat(3),
        );
        for bundle in [diorama_bundle(), hike_bundle(), weathered, hill] {
            let bytes = bundle.to_bytes().unwrap();
            assert_eq!(SceneBundle::from_bytes(&bytes).unwrap(), bundle);
        }
    }

    #[test]
    fn diorama_bundle_has_every_feature() {
        let bundle = diorama_bundle();
        for class in [
            Landcover::Water,
            Landcover::Rock,
            Landcover::Scree,
            Landcover::Coniferous,
            Landcover::Broadleaf,
            Landcover::Grassland,
        ] {
            assert!(
                bundle.terrain.landcover.contains(&u8::from(class)),
                "{class:?}"
            );
        }
        for bit in [line_mask::PATH, line_mask::ROAD, line_mask::STREAM] {
            assert!(bundle.terrain.lines.iter().any(|&l| l & bit != 0), "{bit}");
        }
        let lake = &bundle.water[0];
        assert!(lake.cells.len() > 50);
        for &cell in &lake.cells {
            let height = bundle.terrain.height_m(cell as usize);
            assert!((height - lake.level_m).abs() < 0.01);
        }
    }
}
