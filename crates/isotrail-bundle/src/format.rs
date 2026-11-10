use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec_with_limit;

use crate::{BundleError, Landcover, SceneBundle};

pub const MAGIC: [u8; 4] = *b"ITB1";
/// Bumped whenever the payload layout changes
pub const FORMAT_VERSION: u16 = 5;
const HEADER_LEN: usize = MAGIC.len() + 2;
const COMPRESSION_LEVEL: u8 = 9;
const MAX_PAYLOAD_BYTES: usize = 256 << 20;

pub fn is_bundle(bytes: &[u8]) -> bool {
    bytes.starts_with(&MAGIC)
}

impl SceneBundle {
    /// Magic, little-endian format version, then the deflated postcard payload
    pub fn to_bytes(&self) -> Vec<u8> {
        let payload = postcard::to_allocvec(self).expect("bundles always serialise");
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len() / 2);
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.extend(compress_to_vec(&payload, COMPRESSION_LEVEL));
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<SceneBundle, BundleError> {
        if !is_bundle(bytes) || bytes.len() < HEADER_LEN {
            return Err(BundleError::NotABundle);
        }
        let found = u16::from_le_bytes([bytes[4], bytes[5]]);
        if found != FORMAT_VERSION {
            return Err(BundleError::UnsupportedVersion {
                found,
                supported: FORMAT_VERSION,
            });
        }
        let payload = decompress_to_vec_with_limit(&bytes[HEADER_LEN..], MAX_PAYLOAD_BYTES)
            .map_err(|err| BundleError::Corrupt(format!("could not decompress ({err})")))?;
        let (bundle, rest) = postcard::take_from_bytes::<SceneBundle>(&payload)
            .map_err(|err| BundleError::Corrupt(err.to_string()))?;
        if !rest.is_empty() {
            return Err(BundleError::Corrupt(format!(
                "{} unexpected bytes at the end",
                rest.len()
            )));
        }
        bundle.validate()?;
        Ok(bundle)
    }

    /// Stable hash of the terrain and route, for seeding anything random
    pub fn content_seed(&self) -> u64 {
        let content = (&self.grid, &self.terrain, &self.water, &self.route);
        fnv1a(&postcard::to_allocvec(&content).expect("bundles always serialise"))
    }

    fn validate(&self) -> Result<(), BundleError> {
        let corrupt = |what: String| Err(BundleError::Corrupt(what));
        let cells = self.grid.cells();
        if cells == 0 {
            return corrupt("the grid has no cells".into());
        }
        let terrain = &self.terrain;
        for (layer, len) in [
            ("heights", terrain.heights.len()),
            ("landcover", terrain.landcover.len()),
            ("lines", terrain.lines.len()),
        ] {
            if len != cells {
                return corrupt(format!("{len} {layer} for {cells} cells"));
            }
        }
        if !(terrain.height_scale_m.is_finite() && terrain.height_scale_m > 0.0) {
            return corrupt(format!("height scale {}", terrain.height_scale_m));
        }
        if let Some(&class) = terrain
            .landcover
            .iter()
            .find(|&&c| Landcover::try_from(c).is_err())
        {
            return corrupt(format!("unknown landcover class {class}"));
        }
        if let Some(&cell) = self
            .water
            .iter()
            .flat_map(|body| &body.cells)
            .find(|&&c| c as usize >= cells)
        {
            return corrupt(format!("water in cell {cell} of {cells}"));
        }
        let points = self.route.points.len();
        if let Some(&at) = self.route.breaks.iter().find(|&&b| b as usize >= points) {
            return corrupt(format!("route break at point {at} of {points}"));
        }
        if let Some(sample) = self
            .sun
            .iter()
            .find(|s| !(s.azimuth_deg.is_finite() && (-90.0..=90.0).contains(&s.elevation_deg)))
        {
            return corrupt(format!("sun sample {sample:?}"));
        }
        if self.sun.windows(2).any(|w| w[0].time >= w[1].time) {
            return corrupt("sun samples out of order".into());
        }
        if let Some(sample) = self.moon.iter().find(|s| {
            !(s.azimuth_deg.is_finite()
                && (-90.0..=90.0).contains(&s.elevation_deg)
                && (0.0..=1.0).contains(&s.illuminated))
        }) {
            return corrupt(format!("moon sample {sample:?}"));
        }
        if self.moon.len() != self.sun.len()
            || self
                .moon
                .iter()
                .zip(&self.sun)
                .any(|(m, s)| m.time != s.time)
        {
            return corrupt("moon samples don't match the sun's".into());
        }
        if let Some(weather) = &self.weather {
            if weather.samples.is_empty() {
                return corrupt("weather without samples".into());
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
                return corrupt(format!("weather sample {sample:?}"));
            }
            if weather.samples.windows(2).any(|w| w[0].time >= w[1].time) {
                return corrupt("weather samples out of order".into());
            }
        }
        let season = &self.season;
        if season.leaf_day.is_some_and(|day| !(1..=366).contains(&day))
            || [season.snow_line_m, season.ice_line_m]
                .iter()
                .flatten()
                .any(|line| !line.is_finite())
        {
            return corrupt(format!("season {season:?}"));
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
            water: vec![WaterBody {
                level_m: 402.0,
                cells: vec![0, 1, 6],
            }],
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

    #[test]
    fn round_trips() {
        let bundle = bundle();
        let bytes = bundle.to_bytes();
        assert!(is_bundle(&bytes));
        assert_eq!(&bytes[..6], b"ITB1\x05\x00");
        assert_eq!(SceneBundle::from_bytes(&bytes).unwrap(), bundle);
        assert_eq!(bundle.to_bytes(), bytes, "encoding is deterministic");
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
    fn other_versions_are_refused_with_both_numbers() {
        let mut bytes = bundle().to_bytes();
        bytes[4..6].copy_from_slice(&7u16.to_le_bytes());
        let err = SceneBundle::from_bytes(&bytes).unwrap_err();
        assert!(matches!(
            err,
            BundleError::UnsupportedVersion {
                found: 7,
                supported: FORMAT_VERSION
            }
        ));
        let message = err.to_string();
        assert!(message.contains("version 7"), "{message}");
        assert!(message.contains("isotrail bundle"), "{message}");
    }

    #[test]
    fn bundles_from_older_versions_ask_to_be_made_again() {
        for old in [1u16, 2, 3, 4] {
            let mut bytes = bundle().to_bytes();
            bytes[4..6].copy_from_slice(&old.to_le_bytes());
            let message = SceneBundle::from_bytes(&bytes).unwrap_err().to_string();
            assert!(
                message.contains(&format!("version {old}")) && message.contains("reads version 5"),
                "{message}"
            );
            assert!(message.contains("`isotrail bundle`"), "{message}");
        }
    }

    #[test]
    fn damaged_payloads_are_reported() {
        let bytes = bundle().to_bytes();
        for damaged in [
            bytes[..bytes.len() / 2].to_vec(),
            [&bytes[..6], &[0xff; 40][..]].concat(),
            [&bytes[..6], &compress_to_vec(b"short", 9)[..]].concat(),
        ] {
            assert!(
                matches!(
                    SceneBundle::from_bytes(&damaged),
                    Err(BundleError::Corrupt(_))
                ),
                "{damaged:?}"
            );
        }
    }

    #[test]
    fn inconsistent_contents_are_refused() {
        let mut cases = Vec::new();
        let mut b = bundle();
        b.terrain.heights.pop();
        cases.push((b, "23 heights for 24 cells"));
        let mut b = bundle();
        b.terrain.landcover[5] = 13;
        cases.push((b, "unknown landcover class 13"));
        let mut b = bundle();
        b.water[0].cells.push(24);
        cases.push((b, "water in cell 24"));
        let mut b = bundle();
        b.route.breaks.push(5);
        cases.push((b, "route break at point 5"));
        let mut b = bundle();
        b.terrain.height_scale_m = 0.0;
        cases.push((b, "height scale 0"));
        let mut b = bundle();
        b.sun[2].time = b.sun[1].time;
        cases.push((b, "sun samples out of order"));
        let mut b = bundle();
        b.sun[0].elevation_deg = f32::NAN;
        cases.push((b, "sun sample"));
        let mut b = bundle();
        b.moon[1].illuminated = 1.5;
        cases.push((b, "moon sample"));
        let mut b = bundle();
        b.moon.pop();
        cases.push((b, "moon samples don't match the sun's"));
        let mut b = bundle();
        b.moon[2].time += 1;
        cases.push((b, "moon samples don't match the sun's"));
        let mut b = bundle();
        b.weather.as_mut().unwrap().samples[1].cloud_low_pct = 101.0;
        cases.push((b, "weather sample"));
        let mut b = bundle();
        b.weather.as_mut().unwrap().samples[0].rain_mm = -0.1;
        cases.push((b, "weather sample"));
        let mut b = bundle();
        b.weather.as_mut().unwrap().samples[0].wind_direction_deg = f32::NAN;
        cases.push((b, "weather sample"));
        let mut b = bundle();
        b.weather.as_mut().unwrap().samples[1].time -= 3600;
        cases.push((b, "weather samples out of order"));
        let mut b = bundle();
        b.weather.as_mut().unwrap().samples.clear();
        cases.push((b, "weather without samples"));
        let mut b = bundle();
        b.season.leaf_day = Some(0);
        cases.push((b, "season"));
        let mut b = bundle();
        b.season.ice_line_m = Some(f32::INFINITY);
        cases.push((b, "season"));
        for (bundle, expected) in cases {
            let message = SceneBundle::from_bytes(&bundle.to_bytes())
                .unwrap_err()
                .to_string();
            assert!(message.contains(expected), "{message}");
        }
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
    fn fnv1a_matches_reference_values() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
