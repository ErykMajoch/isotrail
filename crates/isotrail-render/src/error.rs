use crate::AdapterMode;

/// Why the GPU could not be set up or a frame could not be rendered
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RenderError {
    #[error("unknown ISOTRAIL_GPU value {0:?}: expected auto, hardware or software")]
    UnknownAdapterMode(String),

    #[error("no {mode} GPU adapter found; set ISOTRAIL_GPU=software to use a software renderer")]
    NoAdapter { mode: AdapterMode },

    #[error("this browser offers neither WebGPU nor WebGL2")]
    NoBrowserAdapter,

    #[error("could not draw into the canvas")]
    Surface(#[from] wgpu::CreateSurfaceError),

    #[error("could not open the GPU device")]
    Device(#[from] wgpu::RequestDeviceError),

    #[error("GPU did not finish rendering")]
    Poll(#[from] wgpu::PollError),

    #[error("could not read the rendered frame back from the GPU")]
    Readback,

    #[error(
        "the terrain needs {needed} of GPU memory, more than this GPU allows ({allowed_mb}MB); \
         lower --exaggeration or --detail, or raise --block-height"
    )]
    MeshTooLarge { needed: String, allowed_mb: u64 },
}
