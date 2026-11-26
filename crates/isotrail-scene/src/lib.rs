//! WebAssembly entry point that builds a scene file from a GPX file in a
//! browser worker. The page fetches what the elevation, map and weather
//! services answer, and the scene is built from those answers

mod build;
#[cfg(target_arch = "wasm32")]
mod builder;
mod notes;
mod page;
mod servers;

pub use build::{
    Builder, Failure, Kind, MAP_DATA_BUSY_WAITS, MAX_MAP_DATA_KM2, Outcome, Wait, waits,
};
#[cfg(target_arch = "wasm32")]
pub use builder::{SceneBuilder, Wanted, load_timezones, offline};
pub use page::{adjust, check_palette, check_theme, read_options};
pub use servers::{SERVER_NAMES, VK_MAPS_URL, map_data_servers, server_name};
