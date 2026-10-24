use std::path::PathBuf;

/// Why a frame could not be exported
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    #[error("the upscale factor must be at least 1")]
    ZeroScale,

    #[error("could not encode PNG")]
    Png(#[from] ::png::EncodingError),

    #[error("could not write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: Box<ExportError>,
    },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
