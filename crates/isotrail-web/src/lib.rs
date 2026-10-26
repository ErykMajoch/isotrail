//! WebAssembly entry point for the `<isotrail-hike>` web embed: loads a scene
//! bundle and drives the renderer on a canvas

#[cfg(target_arch = "wasm32")]
mod viewer;

#[cfg(target_arch = "wasm32")]
pub use viewer::Viewer;
