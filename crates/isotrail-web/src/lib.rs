//! WebAssembly entry point for the `<isotrail-hike>` web embed: loads a scene
//! bundle and drives the renderer on a canvas

#[cfg(all(target_arch = "wasm32", feature = "export"))]
mod export;
#[cfg(target_arch = "wasm32")]
mod options;
#[cfg(target_arch = "wasm32")]
mod viewer;

#[cfg(all(target_arch = "wasm32", feature = "export"))]
pub use export::{ClipRead, StillRead};
#[cfg(target_arch = "wasm32")]
pub use viewer::Viewer;
