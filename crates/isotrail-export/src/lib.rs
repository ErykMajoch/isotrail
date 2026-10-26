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
pub use gif::{GifWriter, MAX_GIF_FPS};
pub use mp4::{Ffmpeg, Mp4Writer};
pub use png::{encode_png, write_png};
pub use still::{DEFAULT_SCALE, render_still};
pub use upscale::upscale;
