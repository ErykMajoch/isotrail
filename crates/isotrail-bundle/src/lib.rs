//! The scene bundle: everything needed to render a hike diorama, resolved
//! ahead of time and stored in a versioned compressed binary format (`.itb`)
//!
//! This crate is wasm-safe: no filesystem, network or thread access

mod error;
mod format;
mod landcover;
mod previous;
mod scene;
mod season;
mod units;
mod weather;

pub use error::BundleError;
pub use format::{
    FORMAT_VERSION, MAGIC, MAX_GRID_SIDE, OLDEST_FORMAT_VERSION, format_version, is_bundle,
};
pub use landcover::{Landcover, line_mask};
pub use scene::{
    Bounds, GridInfo, HikeStats, Metadata, MoonSample, ProfileSample, Provenance, Route,
    RoutePoint, SceneBundle, SunSample, Terrain, WaterBody,
};
pub use season::Season;
pub use units::Units;
pub use weather::{
    Precipitation, WEATHER_ATTRIBUTION, Weather, WeatherDataset, WeatherSample, WeatherState,
    weather_condition,
};
