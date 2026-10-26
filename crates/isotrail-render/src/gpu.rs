use std::fmt;
use std::str::FromStr;

use crate::RenderError;

/// Environment variable that picks the GPU adapter: `auto`, `hardware` or `software`
pub const ADAPTER_ENV: &str = "ISOTRAIL_GPU";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdapterMode {
    /// A real GPU when there is one, otherwise the software fallback
    #[default]
    Auto,
    Hardware,
    /// The platform's software rasteriser (WARP on Windows, lavapipe or llvmpipe on Linux)
    Software,
}

impl AdapterMode {
    pub fn from_env() -> Result<Self, RenderError> {
        match std::env::var(ADAPTER_ENV) {
            Ok(value) => value.parse(),
            Err(_) => Ok(Self::default()),
        }
    }
}

impl FromStr for AdapterMode {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Self::Auto),
            "hardware" => Ok(Self::Hardware),
            "software" => Ok(Self::Software),
            _ => Err(RenderError::UnknownAdapterMode(s.to_owned())),
        }
    }
}

impl fmt::Display for AdapterMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Hardware => "hardware",
            Self::Software => "software",
        })
    }
}

pub struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_info: wgpu::AdapterInfo,
}

impl Gpu {
    pub async fn new(mode: AdapterMode) -> Result<Self, RenderError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = request_adapter(&instance, mode, None).await?;
        Self::open(adapter).await
    }

    /// Opens the GPU that draws into `canvas`, on WebGPU where the browser
    /// offers it and WebGL2 otherwise, or on WebGL2 alone with `webgl_only`
    #[cfg(target_arch = "wasm32")]
    pub async fn for_canvas(
        canvas: web_sys::HtmlCanvasElement,
        webgl_only: bool,
    ) -> Result<(Self, wgpu::Surface<'static>), RenderError> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = if webgl_only {
            wgpu::Backends::GL
        } else {
            wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
        };
        let instance = wgpu::util::new_instance_with_webgpu_detection(descriptor).await;
        // WebGL2 ties the adapter to the canvas, so the surface comes first
        let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas))?;
        let adapter = request_adapter(&instance, AdapterMode::Auto, Some(&surface))
            .await
            .map_err(|_| RenderError::NoBrowserAdapter)?;
        Ok((Self::open(adapter).await?, surface))
    }

    async fn open(adapter: wgpu::Adapter) -> Result<Self, RenderError> {
        let adapter_info = adapter.get_info();
        tracing::info!(
            adapter = %adapter_info.name,
            backend = %adapter_info.backend,
            device_type = ?adapter_info.device_type,
            "using GPU adapter"
        );
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("isotrail"),
                // The same limits everywhere, so native runs catch what WebGL2 can't do
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await?;
        Ok(Self {
            adapter,
            device,
            queue,
            adapter_info,
        })
    }

    /// Opens a GPU without a window, honouring [`ADAPTER_ENV`]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless() -> Result<Self, RenderError> {
        pollster::block_on(Self::new(AdapterMode::from_env()?))
    }

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }
}

async fn request_adapter(
    instance: &wgpu::Instance,
    mode: AdapterMode,
    compatible_surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, RenderError> {
    let request = |force_fallback_adapter| {
        instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter,
            compatible_surface,
            ..Default::default()
        })
    };
    let adapter = match mode {
        AdapterMode::Auto => match request(false).await {
            Ok(adapter) => Some(adapter),
            Err(error) => {
                tracing::warn!(%error, "no hardware GPU adapter, trying the software fallback");
                request(true).await.ok()
            }
        },
        // A non-fallback request can still return a CPU device when it is the only one
        AdapterMode::Hardware => request(false)
            .await
            .ok()
            .filter(|adapter| adapter.get_info().device_type != wgpu::DeviceType::Cpu),
        AdapterMode::Software => request(true).await.ok(),
    };
    adapter.ok_or(RenderError::NoAdapter { mode })
}
