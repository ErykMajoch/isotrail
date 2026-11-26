use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};

use crate::post::GBuffer;
use crate::{Frame, Gpu, RenderError};

/// Not sRGB, so the bytes read back are exactly what the shaders wrote
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A colour texture at internal resolution that can be read back to the CPU,
/// with the intermediate targets that drawing into it needs. Make one per
/// frame size and reuse it, as a browser only frees its GPU memory on drop
pub struct OffscreenTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    gbuffer: GBuffer,
    /// Made on the first read and kept for the next
    staging: OnceLock<wgpu::Buffer>,
}

type Mapped = Arc<Mutex<Option<Result<(), wgpu::BufferAsyncError>>>>;

impl OffscreenTarget {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            gbuffer: GBuffer::new(gpu, width, height),
            staging: OnceLock::new(),
        }
    }

    pub fn width(&self) -> u32 {
        self.texture.width()
    }

    pub fn height(&self) -> u32 {
        self.texture.height()
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub(crate) fn gbuffer(&self) -> &GBuffer {
        &self.gbuffer
    }

    /// Copies the texture to the CPU, blocking until the GPU has finished
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_back(&self, gpu: &Gpu) -> Result<Frame, RenderError> {
        let (buffer, mapped) = self.copy_out(gpu);
        gpu.device().poll(wgpu::PollType::wait_indefinitely())?;
        let result = mapped.lock().map_err(|_| RenderError::Readback)?.take();
        result
            .ok_or(RenderError::Readback)?
            .map_err(|_| RenderError::Readback)?;
        self.frame_from(buffer)
    }

    /// Copies the texture to the CPU without blocking. Between checks it
    /// awaits `pause`, which must let the GPU's work finish, such as a
    /// browser task. One read per target at a time
    pub async fn read_back_async<F: Future<Output = ()>>(
        &self,
        gpu: &Gpu,
        mut pause: impl FnMut() -> F,
    ) -> Result<Frame, RenderError> {
        let (buffer, mapped) = self.copy_out(gpu);
        loop {
            gpu.device().poll(wgpu::PollType::Poll)?;
            let result = mapped.lock().map_err(|_| RenderError::Readback)?.take();
            if let Some(result) = result {
                result.map_err(|_| RenderError::Readback)?;
                return self.frame_from(buffer);
            }
            pause().await;
        }
    }

    fn padded_row_bytes(&self) -> u32 {
        (self.width() * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
    }

    /// Copies the texture into the staging buffer and asks to map it
    fn copy_out(&self, gpu: &Gpu) -> (&wgpu::Buffer, Mapped) {
        let padded_row_bytes = self.padded_row_bytes();
        let buffer = self.staging.get_or_init(|| {
            gpu.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size: u64::from(padded_row_bytes) * u64::from(self.height()),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });
        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(self.height()),
                },
            },
            self.texture.size(),
        );
        gpu.queue().submit([encoder.finish()]);

        let mapped: Mapped = Arc::default();
        let done = mapped.clone();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            if let Ok(mut done) = done.lock() {
                *done = Some(result);
            }
        });
        (buffer, mapped)
    }

    /// The mapped buffer's rows without their padding; unmaps it
    fn frame_from(&self, buffer: &wgpu::Buffer) -> Result<Frame, RenderError> {
        let (width, height) = (self.width(), self.height());
        let row_bytes = width as usize * 4;
        let mut rgba = Vec::with_capacity(row_bytes * height as usize);
        {
            let mapped = buffer
                .get_mapped_range(..)
                .map_err(|_| RenderError::Readback)?;
            for row in mapped.chunks_exact(self.padded_row_bytes() as usize) {
                rgba.extend_from_slice(&row[..row_bytes]);
            }
        }
        buffer.unmap();
        Ok(Frame::new(width, height, rgba))
    }
}

impl Drop for OffscreenTarget {
    fn drop(&mut self) {
        self.texture.destroy();
        if let Some(buffer) = self.staging.get() {
            buffer.destroy();
        }
    }
}
