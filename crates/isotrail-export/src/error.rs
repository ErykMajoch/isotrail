use std::path::PathBuf;

/// Why a frame could not be exported
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    #[error("the upscale factor must be at least 1")]
    ZeroScale,

    #[error(transparent)]
    Render(#[from] isotrail_render::RenderError),

    #[error("could not encode PNG")]
    Png(#[from] ::png::EncodingError),

    #[error("could not encode GIF")]
    Gif(#[from] ::gif::EncodingError),

    #[error("GIF output needs a palette; choose one with --palette instead of none")]
    GifPalette,

    #[error("a GIF can have at most 255 colours and at least 1, not {0}")]
    GifColours(usize),

    #[error("a GIF can show at most 50 frames a second, not {0}")]
    GifFps(u32),

    #[error("a GIF can be at most 65535 pixels wide and high, not {width} × {height}")]
    GifSize { width: u32, height: u32 },

    #[error("expected a {} × {} frame, got {} × {}", expected.0, expected.1, actual.0, actual.1)]
    FrameSize {
        expected: (u32, u32),
        actual: (u32, u32),
    },

    #[error("could not write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: Box<ExportError>,
    },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
