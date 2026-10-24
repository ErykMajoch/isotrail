//! The scene bundle: everything needed to render a hike diorama, resolved
//! ahead of time and stored in a versioned compressed binary format (`.itb`)
//!
//! This crate is wasm-safe: no filesystem, network or thread access

mod error;
mod format;
mod landcover;
mod scene;

pub use error::BundleError;
pub use format::{FORMAT_VERSION, MAGIC, is_bundle};
pub use landcover::{Landcover, line_mask};
pub use scene::{
    Bounds, GridInfo, HikeStats, Metadata, ProfileSample, Provenance, Route, RoutePoint,
    SceneBundle, Terrain, WaterBody,
};
