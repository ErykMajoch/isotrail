//! Export for isotrail: the headless frame loop and the PNG, GIF and MP4
//! writers

mod animate;
mod error;
mod gif;
mod mp4;
mod png;
mod still;
mod upscale;

pub use animate::render_clip;
pub use error::ExportError;
pub use gif::{GifOptions, GifWriter, MAX_GIF_FPS, Plays, check_gif_look};
pub use mp4::{DEFAULT_CRF, Ffmpeg, MAX_CRF, MAX_MP4_FPS, Mp4Options, Mp4Writer, X264_PRESETS};
pub use png::{encode_png, write_png};
pub use still::{DEFAULT_SCALE, render_still};
pub use upscale::upscale;
