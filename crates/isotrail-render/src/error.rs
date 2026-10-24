use crate::AdapterMode;

/// Why the GPU could not be set up or a frame could not be rendered
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RenderError {
    #[error("unknown ISOTRAIL_GPU value {0:?}: expected auto, hardware or software")]
    UnknownAdapterMode(String),

    #[error("no {mode} GPU adapter found; set ISOTRAIL_GPU=software to use a software renderer")]
    NoAdapter { mode: AdapterMode },

    #[error("could not open the GPU device")]
    Device(#[from] wgpu::RequestDeviceError),

    #[error("GPU did not finish rendering")]
    Poll(#[from] wgpu::PollError),

    #[error("could not read the rendered frame back from the GPU")]
    Readback,
}
