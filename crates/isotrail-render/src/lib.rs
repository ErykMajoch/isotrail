//! The isotrail renderer: terrain meshers, camera, render passes and overlays
//! that turn a scene bundle into pixel-art frames, natively or in the browser
//!
//! This crate is wasm-safe: no filesystem, network or thread access

mod error;
mod frame;
mod gpu;
mod target;
mod test_scene;

pub use error::RenderError;
pub use frame::Frame;
pub use gpu::{ADAPTER_ENV, AdapterMode, Gpu};
pub use target::{OffscreenTarget, TARGET_FORMAT};
pub use test_scene::draw_test_scene;
