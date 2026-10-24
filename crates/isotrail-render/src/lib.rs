//! The isotrail renderer: terrain meshers, camera, render passes and overlays
//! that turn a scene bundle into pixel-art frames, natively or in the browser
//!
//! This crate is wasm-safe: no filesystem, network or thread access

mod camera;
mod error;
mod frame;
mod gpu;
mod mesh;
mod target;
mod terrain;
mod test_scene;

pub use camera::{
    Aspect, Camera, DEFAULT_YAW_OFFSET_DEG, Framing, Insets, PITCH_DEG, route_heading, view_heading,
};

pub use error::RenderError;
pub use frame::Frame;
pub use gpu::{ADAPTER_ENV, AdapterMode, Gpu};
pub use mesh::{Mesher, SmoothMesher, TerrainMesh, Vertex};
pub use target::{OffscreenTarget, TARGET_FORMAT};
pub use terrain::{
    AMBIENT, BACKGROUND, GpuMesh, Lighting, SUN_AZIMUTH_DEG, SUN_ELEVATION_DEG, TerrainRenderer,
};
pub use test_scene::draw_test_scene;
