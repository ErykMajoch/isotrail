//! Export for isotrail: finished stills and clips with their text, the
//! headless frame loop and the PNG, GIF and MP4 writers

#[cfg(not(target_arch = "wasm32"))]
mod animate;
mod compose;
mod error;
mod gif;
mod mp4;
mod png;
#[cfg(not(target_arch = "wasm32"))]
mod still;
mod upscale;

#[cfg(not(target_arch = "wasm32"))]
pub use animate::render_clip;
#[cfg(not(target_arch = "wasm32"))]
pub use compose::finished_clip;
pub use compose::{Caption, Still};
pub use error::ExportError;
pub use gif::{GifOptions, GifWriter, MAX_GIF_FPS, Plays, check_gif_look};
pub use mp4::{DEFAULT_CRF, Ffmpeg, MAX_CRF, MAX_MP4_FPS, Mp4Options, Mp4Writer, X264_PRESETS};
pub use png::{encode_png, write_png};
#[cfg(not(target_arch = "wasm32"))]
pub use still::render_still;
pub use upscale::{DEFAULT_SCALE, upscale};
