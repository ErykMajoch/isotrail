use crate::Gpu;
#[cfg(not(target_arch = "wasm32"))]
use crate::{Frame, RenderError};

/// Not sRGB, so the bytes read back are exactly what the shaders wrote
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A colour texture at internal resolution that can be read back to the CPU
pub struct OffscreenTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view }
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

    /// Copies the texture to the CPU, blocking until the GPU has finished
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_back(&self, gpu: &Gpu) -> Result<Frame, RenderError> {
        let (width, height) = (self.width(), self.height());
        let row_bytes = width * 4;
        let padded_row_bytes = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

        let buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded_row_bytes) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(height),
                },
            },
            self.texture.size(),
        );
        gpu.queue().submit([encoder.finish()]);

        let (sender, receiver) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        gpu.device().poll(wgpu::PollType::wait_indefinitely())?;
        receiver
            .recv()
            .map_err(|_| RenderError::Readback)?
            .map_err(|_| RenderError::Readback)?;

        let mut rgba = Vec::with_capacity(row_bytes as usize * height as usize);
        {
            let mapped = buffer
                .get_mapped_range(..)
                .map_err(|_| RenderError::Readback)?;
            for row in mapped.chunks_exact(padded_row_bytes as usize) {
                rgba.extend_from_slice(&row[..row_bytes as usize]);
            }
        }
        buffer.unmap();
        Ok(Frame::new(width, height, rgba))
    }
}
