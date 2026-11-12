use std::f64::consts::TAU;
use std::ops::RangeInclusive;

use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec_with_limit;

use crate::{BundleError, Landcover, SceneBundle};

pub const MAGIC: [u8; 4] = *b"ITB1";
/// Bumped whenever the payload layout changes
pub const FORMAT_VERSION: u16 = 6;
pub const MAX_GRID_SIDE: u32 = 1024;
const HEADER_LEN: usize = MAGIC.len() + 2;
const COMPRESSION_LEVEL: u8 = 9;
const MAX_PAYLOAD_BYTES: usize = 256 << 20;
const MAX_CELL_SIZE_M: f64 = 1e6;
const EXAGGERATION: RangeInclusive<f64> = 0.1..=100.0;
/// More than the drop from the highest peak to the deepest trench
const MAX_RELIEF_M: f32 = 20_000.0;
const DAMAGED: &str = "its data is cut short or garbled";
/// 200 years, the longest a hike's times may span
const MAX_SPAN_S: i64 = 6_311_520_000;
/// Unix seconds from the year 0 to the end of 9999, the years a GPX time can hold
const INSTANTS: RangeInclusive<i64> = -62_167_219_200..=253_402_300_799;

pub fn is_bundle(bytes: &[u8]) -> bool {
    bytes.starts_with(&MAGIC)
}

impl SceneBundle {
    /// Magic, little-endian format version, then the deflated postcard
    /// payload. Refuses a scene that [`SceneBundle::from_bytes`] would refuse
    pub fn to_bytes(&self) -> Result<Vec<u8>, BundleError> {
        self.validate()?;
        let payload = postcard::to_allocvec(self).expect("bundles always serialise");
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len() / 2);
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.extend(compress_to_vec(&payload, COMPRESSION_LEVEL));
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<SceneBundle, BundleError> {
        if !is_bundle(bytes) || bytes.len() < HEADER_LEN {
            return Err(BundleError::NotABundle);
        }
        let found = u16::from_le_bytes([bytes[4], bytes[5]]);
        let supported = FORMAT_VERSION;
        if found < supported {
            return Err(BundleError::OlderVersion { found, supported });
        }
        if found > supported {
            return Err(BundleError::NewerVersion { found, supported });
        }
        let damaged = || BundleError::Corrupt(DAMAGED.into());
        let payload = decompress_to_vec_with_limit(&bytes[HEADER_LEN..], MAX_PAYLOAD_BYTES)
            .map_err(|_| damaged())?;
        let (bundle, rest) =
            postcard::take_from_bytes::<SceneBundle>(&payload).map_err(|_| damaged())?;
        if !rest.is_empty() {
            return Err(damaged());
        }
        bundle.check().map_err(BundleError::Corrupt)?;
        Ok(bundle)
    }

    /// Stable hash of the terrain and route, for seeding anything random;
    /// water is hashed without its sea flag, so seeds stay as they were
    /// before bundles had one
    pub fn content_seed(&self) -> u64 {
        let water: Vec<(f32, &[u32])> = self
            .water
            .iter()
            .map(|body| (body.level_m, &body.cells[..]))
            .collect();
        let content = (&self.grid, &self.terrain, water, &self.route);
        fnv1a(&postcard::to_allocvec(&content).expect("bundles always serialise"))
    }

    /// Refuses a scene that [`SceneBundle::from_bytes`] would refuse
    pub fn validate(&self) -> Result<(), BundleError> {
        self.check().map_err(BundleError::Invalid)
    }

    fn check(&self) -> Result<(), String> {
        self.validate_grid()?;
        self.validate_route()?;
        self.validate_sky_and_weather()?;
        let season = &self.season;
        if season.leaf_day.is_some_and(|day| !(1..=366).contains(&day))
            || [season.snow_line_m, season.ice_line_m]
                .iter()
                .flatten()
                .any(|line| !line.is_finite())
        {
            return Err(format!("season {season:?}"));
        }
        Ok(())
    }

    fn validate_grid(&self) -> Result<(), String> {
        let grid = &self.grid;
        let sides = 1..=MAX_GRID_SIDE;
        if !(sides.contains(&grid.cols) && sides.contains(&grid.rows)) {
            return Err(format!(
                "a grid of {} x {} cells (each side takes 1 to {MAX_GRID_SIDE})",
                grid.cols, grid.rows
            ));
        }
        if !(grid.cell_size_m > 0.0 && grid.cell_size_m <= MAX_CELL_SIZE_M) {
            return Err(format!("cell size {}m", grid.cell_size_m));
        }
        if !EXAGGERATION.contains(&grid.exaggeration) {
            return Err(format!("exaggeration {}", grid.exaggeration));
        }
        if !(grid.rotation_rad.is_finite() && grid.rotation_rad.abs() <= TAU) {
            return Err(format!("rotation {}", grid.rotation_rad));
        }
        if !((-90.0..=90.0).contains(&grid.origin_lat)
            && (-180.0..=180.0).contains(&grid.origin_lon)
            && grid.center_m.0.is_finite()
            && grid.center_m.1.is_finite())
        {
            return Err(format!(
                "grid origin {}, {} and centre {:?}",
                grid.origin_lat, grid.origin_lon, grid.center_m
            ));
        }
        let cells = grid.cells();
        let terrain = &self.terrain;
        for (layer, len) in [
            ("heights", terrain.heights.len()),
            ("landcover", terrain.landcover.len()),
            ("lines", terrain.lines.len()),
        ] {
            if len != cells {
                return Err(format!("{len} {layer} for {cells} cells"));
            }
        }
        if !(terrain.height_scale_m > 0.0
            && terrain.height_scale_m * f32::from(u16::MAX) <= MAX_RELIEF_M)
        {
            return Err(format!("height scale {}", terrain.height_scale_m));
        }
        if !terrain.height_offset_m.is_finite() {
            return Err(format!("height offset {}", terrain.height_offset_m));
        }
        if let Some(&class) = terrain
            .landcover
            .iter()
            .find(|&&c| Landcover::try_from(c).is_err())
        {
            return Err(format!("unknown landcover class {class}"));
        }
        for body in &self.water {
            if !body.level_m.is_finite() {
                return Err(format!("water level {}", body.level_m));
            }
            if let Some(&cell) = body.cells.iter().find(|&&c| c as usize >= cells) {
                return Err(format!("water in cell {cell} of {cells}"));
            }
        }
        Ok(())
    }

    fn validate_route(&self) -> Result<(), String> {
        let points = &self.route.points;
        if let Some(&at) = self
            .route
            .breaks
            .iter()
            .find(|&&b| b == 0 || b as usize >= points.len())
        {
            return Err(format!("route break at point {at} of {}", points.len()));
        }
        if self.route.breaks.windows(2).any(|w| w[0] >= w[1]) {
            return Err("route breaks out of order".into());
        }
        let (cols, rows) = (self.grid.cols as f32, self.grid.rows as f32);
        if let Some(point) = points.iter().find(|p| {
            !((0.0..=cols).contains(&p.col)
                && (0.0..=rows).contains(&p.row)
                && p.distance_m.is_finite()
                && p.draped_m.is_finite()
                && p.gps_ele_m.is_none_or(f32::is_finite))
        }) {
            return Err(format!("route point {point:?}"));
        }
        if let Some(sample) = self
            .profile
            .iter()
            .find(|s| !(s.distance_m.is_finite() && s.ele_m.is_finite()))
        {
            return Err(format!("profile sample {sample:?}"));
        }
        let stats = &self.meta.stats;
        let bounds = &stats.bounds;
        let numbers = [
            stats.distance_m,
            bounds.min_lat,
            bounds.min_lon,
            bounds.max_lat,
            bounds.max_lon,
        ];
        let optional = [stats.gain_m, stats.loss_m, stats.min_ele_m, stats.max_ele_m];
        if !numbers
            .iter()
            .chain(optional.iter().flatten())
            .all(|v| v.is_finite())
        {
            return Err("statistics that aren't numbers".into());
        }
        for (what, seconds) in [("elapsed", stats.elapsed_s), ("moving", stats.moving_s)] {
            if let Some(seconds) = seconds.filter(|s| !(0..=MAX_SPAN_S).contains(s)) {
                return Err(format!("{what} time of {seconds}s"));
            }
        }
        for (what, time) in [("start", stats.start), ("end", stats.end)] {
            if let Some(time) = time.filter(|t| !INSTANTS.contains(t)) {
                return Err(format!("{what} time {time}"));
            }
        }
        let times: Vec<i64> = points.iter().filter_map(|p| p.time).collect();
        if let Some(time) = times.iter().find(|t| !INSTANTS.contains(t)) {
            return Err(format!("route time {time}"));
        }
        if let (Some(first), Some(last)) = (times.iter().min(), times.iter().max())
            && last - first > MAX_SPAN_S
        {
            return Err(format!("route times {}s apart", last - first));
        }
        Ok(())
    }

    fn validate_sky_and_weather(&self) -> Result<(), String> {
        if let Some(sample) = self
            .sun
            .iter()
            .find(|s| !(s.azimuth_deg.is_finite() && (-90.0..=90.0).contains(&s.elevation_deg)))
        {
            return Err(format!("sun sample {sample:?}"));
        }
        if self.sun.windows(2).any(|w| w[0].time >= w[1].time) {
            return Err("sun samples out of order".into());
        }
        if let Some(sample) = self.moon.iter().find(|s| {
            !(s.azimuth_deg.is_finite()
                && (-90.0..=90.0).contains(&s.elevation_deg)
                && (0.0..=1.0).contains(&s.illuminated))
        }) {
            return Err(format!("moon sample {sample:?}"));
        }
        if self.moon.len() != self.sun.len()
            || self
                .moon
                .iter()
                .zip(&self.sun)
                .any(|(m, s)| m.time != s.time)
        {
            return Err("moon samples don't match the sun's".into());
        }
        if let Some(weather) = &self.weather {
            if weather.samples.is_empty() {
                return Err("weather without samples".into());
            }
            let percent = 0.0..=100.0;
            if let Some(sample) = weather.samples.iter().find(|s| {
                !(s.temperature_c.is_finite()
                    && [
                        s.cloud_cover_pct,
                        s.cloud_low_pct,
                        s.cloud_mid_pct,
                        s.cloud_high_pct,
                    ]
                    .iter()
                    .all(|c| percent.contains(c))
                    && [
                        s.precipitation_mm,
                        s.rain_mm,
                        s.snowfall_cm,
                        s.snow_depth_m,
                        s.wind_speed_m_s,
                    ]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.0)
                    && (0.0..=360.0).contains(&s.wind_direction_deg))
            }) {
                return Err(format!("weather sample {sample:?}"));
            }
            if weather.samples.windows(2).any(|w| w[0].time >= w[1].time) {
                return Err("weather samples out of order".into());
            }
        }
        Ok(())
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Bounds, GridInfo, HikeStats, Metadata, MoonSample, ProfileSample, Provenance, Route,
        RoutePoint, Season, SunSample, Terrain, WaterBody, Weather, WeatherDataset, WeatherSample,
        line_mask,
    };

    fn bundle() -> SceneBundle {
        let (cols, rows) = (6, 4);
        let heights: Vec<f32> = (0..cols * rows).map(|i| 400.0 + i as f32 * 3.5).collect();
        let mut terrain = Terrain::quantise(&heights);
        terrain.landcover = (0..cols * rows).map(|i| (i % 13) as u8).collect();
        terrain.lines = vec![0; (cols * rows) as usize];
        terrain.lines[7] = line_mask::PATH | line_mask::STREAM;
        let point = |i: u16| RoutePoint {
            col: 0.5 + f32::from(i),
            row: 1.25,
            distance_m: 120.0 * f32::from(i),
            time: Some(1_750_000_000 + i64::from(i) * 60),
            gps_ele_m: (i != 2).then_some(410.0 + f32::from(i)),
            draped_m: 405.5,
        };
        SceneBundle {
            meta: Metadata {
                title: "Synthetic Łąka | Ridge".into(),
                name: Some("Synthetic Łąka | Ridge".into()),
                desc: None,
                timezone: Some("Europe/Warsaw".into()),
                local_date: Some("2025-06-15".into()),
                stats: HikeStats {
                    points: 5,
                    segments: 2,
                    distance_m: 480.25,
                    gain_m: Some(12.0),
                    loss_m: Some(3.0),
                    min_ele_m: Some(410.0),
                    max_ele_m: Some(414.0),
                    bounds: Bounds {
                        min_lat: 46.5,
                        min_lon: 10.0,
                        max_lat: 46.51,
                        max_lon: 10.02,
                    },
                    start: Some(1_750_000_000),
                    end: Some(1_750_000_240),
                    elapsed_s: Some(240),
                    moving_s: None,
                },
                attribution: vec!["© OpenStreetMap contributors".into()],
                provenance: Provenance {
                    generator: "isotrail test".into(),
                    elevation: "synthetic".into(),
                    map_data: "synthetic".into(),
                    weather: Some("synthetic".into()),
                },
            },
            grid: GridInfo {
                cols,
                rows,
                cell_size_m: 30.0,
                origin_lat: 46.505,
                origin_lon: 10.01,
                center_m: (12.0, -3.5),
                rotation_rad: 0.4,
                exaggeration: 2.5,
            },
            terrain,
            water: vec![
                WaterBody {
                    level_m: 402.0,
                    cells: vec![0, 1, 6],
                    sea: false,
                },
                WaterBody {
                    level_m: 0.0,
                    cells: vec![23],
                    sea: true,
                },
            ],
            prop_seed: 42,
            route: Route {
                points: (0..5).map(point).collect(),
                breaks: vec![3],
            },
            profile: vec![
                ProfileSample {
                    distance_m: 0.0,
                    ele_m: 410.0,
                },
                ProfileSample {
                    distance_m: 480.25,
                    ele_m: 414.0,
                },
            ],
            sun: (0..3)
                .map(|i| SunSample {
                    time: 1_750_000_000 + i * 120,
                    azimuth_deg: 101.5 + i as f32,
                    elevation_deg: -0.5 + i as f32 * 0.4,
                })
                .collect(),
            moon: (0..3)
                .map(|i| MoonSample {
                    time: 1_750_000_000 + i * 120,
                    azimuth_deg: 250.0 - i as f32,
                    elevation_deg: 30.0 - i as f32 * 0.3,
                    illuminated: 0.62,
                })
                .collect(),
            weather: Some(Weather {
                dataset: WeatherDataset::Archive,
                samples: (0..2)
                    .map(|i| WeatherSample {
                        time: 1_749_999_600 + i * 3600,
                        temperature_c: -1.5 + i as f32,
                        weather_code: 73,
                        cloud_cover_pct: 100.0,
                        cloud_low_pct: 90.0,
                        cloud_mid_pct: 40.0,
                        cloud_high_pct: 0.0,
                        precipitation_mm: 1.4,
                        rain_mm: 0.0,
                        snowfall_cm: 0.98,
                        snow_depth_m: 0.31,
                        wind_speed_m_s: 6.5,
                        wind_direction_deg: 275.0,
                    })
                    .collect(),
            }),
            season: Season {
                leaf_day: Some(166),
                snow_line_m: Some(1450.5),
                ice_line_m: None,
            },
        }
    }

    fn encode_unchecked(bundle: &SceneBundle) -> Vec<u8> {
        let payload = postcard::to_allocvec(bundle).unwrap();
        [
            &MAGIC[..],
            &FORMAT_VERSION.to_le_bytes(),
            &compress_to_vec(&payload, COMPRESSION_LEVEL),
        ]
        .concat()
    }

    #[test]
    fn round_trips() {
        let bundle = bundle();
        let bytes = bundle.to_bytes().unwrap();
        assert!(is_bundle(&bytes));
        assert_eq!(&bytes[..6], b"ITB1\x06\x00");
        assert_eq!(SceneBundle::from_bytes(&bytes).unwrap(), bundle);
        assert_eq!(
            bundle.to_bytes().unwrap(),
            bytes,
            "encoding is deterministic"
        );
        assert_eq!(encode_unchecked(&bundle), bytes);
    }

    #[test]
    fn other_files_are_not_bundles() {
        for bytes in [
            &b""[..],
            b"ITB",
            b"<?xml version=\"1.0\"?><gpx/>",
            b"ITB2\x02\x00",
        ] {
            assert!(
                matches!(SceneBundle::from_bytes(bytes), Err(BundleError::NotABundle)),
                "{bytes:?}"
            );
        }
        assert!(matches!(
            SceneBundle::from_bytes(b"ITB1\x01"),
            Err(BundleError::NotABundle)
        ));
    }

    #[test]
    fn bundles_from_newer_versions_ask_for_an_update() {
        for new in [7u16, 9, u16::MAX] {
            let mut bytes = bundle().to_bytes().unwrap();
            bytes[4..6].copy_from_slice(&new.to_le_bytes());
            let err = SceneBundle::from_bytes(&bytes).unwrap_err();
            assert!(
                matches!(err, BundleError::NewerVersion { found, supported: FORMAT_VERSION } if found == new)
            );
            let message = err.to_string();
            assert!(message.contains("newer isotrail"), "{message}");
            assert!(message.contains(&format!("version {new}")), "{message}");
            assert!(message.contains("update isotrail"), "{message}");
            assert!(!message.contains("isotrail bundle"), "{message}");
        }
    }

    #[test]
    fn bundles_from_older_versions_ask_to_be_made_again() {
        for old in [0u16, 1, 2, 3, 4, 5] {
            let mut bytes = bundle().to_bytes().unwrap();
            bytes[4..6].copy_from_slice(&old.to_le_bytes());
            let err = SceneBundle::from_bytes(&bytes).unwrap_err();
            assert!(matches!(err, BundleError::OlderVersion { .. }));
            let message = err.to_string();
            assert!(message.contains("older isotrail"), "{message}");
            assert!(
                message.contains(&format!("version {old}")) && message.contains("reads version 6"),
                "{message}"
            );
            assert!(message.contains("`isotrail bundle`"), "{message}");
        }
    }

    #[test]
    fn damaged_payloads_say_so_in_plain_words() {
        let bytes = bundle().to_bytes().unwrap();
        let mut garbled_payload = postcard::to_allocvec(&bundle()).unwrap();
        garbled_payload[3] = 0xff;
        for damaged in [
            bytes[..bytes.len() / 2].to_vec(),
            [&bytes[..6], &[0xff; 40][..]].concat(),
            [&bytes[..6], &compress_to_vec(b"short", 9)[..]].concat(),
            [&bytes[..6], &compress_to_vec(&garbled_payload, 9)[..]].concat(),
        ] {
            let message = match SceneBundle::from_bytes(&damaged) {
                Err(err @ BundleError::Corrupt(_)) => err.to_string(),
                other => panic!("{other:?}"),
            };
            assert!(message.starts_with("the bundle is damaged"), "{message}");
            assert!(
                !message.contains("varint") && !message.contains("decompress"),
                "{message}"
            );
        }
    }

    #[test]
    fn unsafe_contents_are_refused_on_read_and_on_write() {
        type Damage = fn(&mut SceneBundle);
        let cases: Vec<(&str, Damage)> = vec![
            ("23 heights for 24 cells", |b| {
                b.terrain.heights.pop();
            }),
            ("unknown landcover class 13", |b| {
                b.terrain.landcover[5] = 13
            }),
            ("water in cell 24", |b| b.water[0].cells.push(24)),
            ("water level", |b| b.water[0].level_m = f32::NAN),
            ("height scale 0", |b| b.terrain.height_scale_m = 0.0),
            ("height scale", |b| b.terrain.height_scale_m = 1.0),
            ("height offset", |b| {
                b.terrain.height_offset_m = f32::NEG_INFINITY;
            }),
            ("a grid of 0 x 4 cells", |b| b.grid.cols = 0),
            ("a grid of 6 x 2048 cells", |b| b.grid.rows = 2048),
            ("cell size", |b| b.grid.cell_size_m = f64::NAN),
            ("cell size", |b| b.grid.cell_size_m = -30.0),
            ("cell size", |b| b.grid.cell_size_m = 1e9),
            ("exaggeration", |b| b.grid.exaggeration = 0.0),
            ("exaggeration", |b| b.grid.exaggeration = 250.0),
            ("exaggeration", |b| b.grid.exaggeration = f64::INFINITY),
            ("rotation", |b| b.grid.rotation_rad = f64::NAN),
            ("rotation", |b| b.grid.rotation_rad = 40.0),
            ("grid origin", |b| b.grid.origin_lat = 91.0),
            ("grid origin", |b| b.grid.center_m.1 = f64::NAN),
            ("route break at point 5", |b| b.route.breaks.push(5)),
            ("route break at point 0", |b| b.route.breaks.insert(0, 0)),
            ("route breaks out of order", |b| b.route.breaks = vec![3, 1]),
            ("route breaks out of order", |b| b.route.breaks = vec![2, 2]),
            ("route point", |b| b.route.points[1].col = f32::NAN),
            ("route point", |b| b.route.points[1].row = -0.5),
            ("route point", |b| b.route.points[4].col = 6.01),
            ("route point", |b| {
                b.route.points[2].draped_m = f32::INFINITY
            }),
            ("route point", |b| b.route.points[2].distance_m = f32::NAN),
            ("route point", |b| {
                b.route.points[3].gps_ele_m = Some(f32::NAN)
            }),
            ("profile sample", |b| b.profile[1].ele_m = f32::NAN),
            ("statistics", |b| b.meta.stats.distance_m = f64::INFINITY),
            ("statistics", |b| b.meta.stats.gain_m = Some(f64::NAN)),
            ("statistics", |b| b.meta.stats.bounds.max_lon = f64::NAN),
            ("elapsed time", |b| b.meta.stats.elapsed_s = Some(i64::MAX)),
            ("elapsed time", |b| b.meta.stats.elapsed_s = Some(-1)),
            ("moving time", |b| {
                b.meta.stats.moving_s = Some(10_000_000_000)
            }),
            ("start time", |b| b.meta.stats.start = Some(i64::MIN)),
            ("end time", |b| b.meta.stats.end = Some(300_000_000_000)),
            ("route time", |b| b.route.points[2].time = Some(i64::MAX)),
            ("route times", |b| {
                b.route.points[4].time = Some(1_750_000_000 + 7_000_000_000)
            }),
            ("sun samples out of order", |b| {
                b.sun[2].time = b.sun[1].time
            }),
            ("sun sample", |b| b.sun[0].elevation_deg = f32::NAN),
            ("moon sample", |b| b.moon[1].illuminated = 1.5),
            ("moon samples don't match the sun's", |b| {
                b.moon.pop();
            }),
            ("moon samples don't match the sun's", |b| {
                b.moon[2].time += 1
            }),
            ("weather sample", |b| {
                b.weather.as_mut().unwrap().samples[1].cloud_low_pct = 101.0;
            }),
            ("weather sample", |b| {
                b.weather.as_mut().unwrap().samples[0].rain_mm = -0.1;
            }),
            ("weather sample", |b| {
                b.weather.as_mut().unwrap().samples[0].wind_direction_deg = f32::NAN;
            }),
            ("weather samples out of order", |b| {
                b.weather.as_mut().unwrap().samples[1].time -= 3600;
            }),
            ("weather without samples", |b| {
                b.weather.as_mut().unwrap().samples.clear();
            }),
            ("season", |b| b.season.leaf_day = Some(0)),
            ("season", |b| b.season.ice_line_m = Some(f32::INFINITY)),
        ];
        for (expected, damage) in cases {
            let mut bundle = bundle();
            damage(&mut bundle);
            let read = SceneBundle::from_bytes(&encode_unchecked(&bundle)).unwrap_err();
            assert!(matches!(read, BundleError::Corrupt(_)), "{read:?}");
            let message = read.to_string();
            assert!(message.contains(expected), "{expected}: {message}");
            let written = bundle.to_bytes().unwrap_err();
            assert!(matches!(written, BundleError::Invalid(_)), "{written:?}");
            let message = written.to_string();
            assert!(
                message.starts_with("the scene can't be saved as a bundle")
                    && message.contains(expected),
                "{expected}: {message}"
            );
        }
    }

    #[test]
    fn values_at_the_edges_of_their_ranges_are_kept() {
        let mut b = bundle();
        b.grid.exaggeration = 100.0;
        b.grid.rotation_rad = -std::f64::consts::PI;
        b.route.points[0].col = 0.0;
        b.route.points[0].row = 0.0;
        b.route.points[4].col = 6.0;
        b.route.points[4].row = 4.0;
        b.route.breaks = vec![1, 3, 4];
        assert_eq!(SceneBundle::from_bytes(&b.to_bytes().unwrap()).unwrap(), b);
    }

    #[test]
    fn seed_follows_the_content_not_the_metadata() {
        let a = bundle();
        let mut b = bundle();
        b.meta.title = "Renamed".into();
        b.prop_seed = 7;
        assert_eq!(a.content_seed(), b.content_seed());
        b.terrain.heights[3] += 1;
        assert_ne!(a.content_seed(), b.content_seed());
        let mut c = bundle();
        c.route.points[1].col += 0.001;
        assert_ne!(a.content_seed(), c.content_seed());
    }

    #[test]
    fn seed_ignores_the_sea_flag_and_matches_bundles_without_one() {
        #[derive(serde::Serialize)]
        struct Before<'a> {
            level_m: f32,
            cells: &'a Vec<u32>,
        }
        let a = bundle();
        let before: Vec<Before> = a
            .water
            .iter()
            .map(|b| Before {
                level_m: b.level_m,
                cells: &b.cells,
            })
            .collect();
        let content = (&a.grid, &a.terrain, before, &a.route);
        assert_eq!(
            a.content_seed(),
            fnv1a(&postcard::to_allocvec(&content).unwrap())
        );
        let mut b = bundle();
        b.water[1].sea = false;
        assert_eq!(a.content_seed(), b.content_seed());
    }

    #[test]
    fn fnv1a_matches_reference_values() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
