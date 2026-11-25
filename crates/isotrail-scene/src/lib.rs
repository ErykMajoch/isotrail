//! WebAssembly entry point that builds a scene file from a GPX file in a
//! browser worker. The page fetches what the elevation, map and weather
//! services answer, and the scene is built from those answers

#[cfg(target_arch = "wasm32")]
mod builder;

#[cfg(target_arch = "wasm32")]
pub use builder::{SceneBuilder, Wanted, load_timezones};
