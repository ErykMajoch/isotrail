//! Track ingest for isotrail: reading GPX files into a clean in-memory track,
//! hike statistics and the timeline used for animation
//!
//! This crate is wasm-safe: no filesystem, network or thread access
//!
//! ```
//! use isotrail_track::{GpxReader, TrackReader};
//!
//! let gpx = br#"<gpx version="1.1"><trk><trkseg>
//!     <trkpt lat="46.5" lon="10.0"><ele>2010.5</ele></trkpt>
//!     <trkpt lat="46.5002" lon="10.0002"><ele>2012.0</ele></trkpt>
//! </trkseg></trk></gpx>"#;
//!
//! let track = GpxReader.parse(gpx)?.clean()?;
//! assert_eq!(track.point_count(), 2);
//! let stats = track.stats()?;
//! assert!((stats.distance_m - 27.0).abs() < 0.1);
//! # Ok::<(), isotrail_track::TrackError>(())
//! ```

mod error;
mod geodesy;
mod gpx;
mod model;
mod reader;
mod simplify;
mod stats;
mod timezone;

pub use chrono_tz::Tz;
pub use error::TrackError;
pub use geodesy::{EARTH_RADIUS_M, haversine_m};
pub use gpx::GpxReader;
pub use isotrail_bundle::Units;
pub use model::{MIN_SEGMENT_POINTS, Segment, Track, TrackPoint};
pub use reader::TrackReader;
pub use stats::{Bounds, GAIN_THRESHOLD_M, PAUSE_THRESHOLD, SMOOTHING_WINDOW_M, TrackStats};
pub use timezone::timezone_at;
