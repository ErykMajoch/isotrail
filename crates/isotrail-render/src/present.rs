use crate::{Gpu, OffscreenTarget};

/// The longest side of a canvas's internal frame, as in a story or wide
/// still at scale 3
pub const MAX_CANVAS_FRAME_PX: u32 = 640;

/// An internal frame for a canvas, and how many canvas pixels each of its
/// pixels covers
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelGrid {
    pub width: u32,
    pub height: u32,
    pub scale: u32,
}

impl PixelGrid {
    /// Pixels as large as keep the long side within [`MAX_CANVAS_FRAME_PX`],
    /// the frame rounded up to cover the whole canvas
    pub fn for_canvas(width: u32, height: u32) -> PixelGrid {
        let scale = width.max(height).div_ceil(MAX_CANVAS_FRAME_PX).max(1);
        PixelGrid {
            width: width.div_ceil(scale).max(1),
            height: height.div_ceil(scale).max(1),
            scale,
        }
    }
}

/// A canvas's drawing buffer shrunk, keeping its shape, until neither side
/// passes `max_side`, the GPU's largest texture
pub fn canvas_within(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= max_side {
        return (width, height);
    }
    let shrink =
        |side: u32| ((u64::from(side) * u64::from(max_side) / u64::from(long)) as u32).max(1);
    (shrink(width), shrink(height))
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    placement: [i32; 4],
}

/// Enlarges a finished frame onto a screen, each pixel a square block
pub struct Present {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    format: wgpu::TextureFormat,
}

impl Present {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Present {
        let device = gpu.device();
        let module = device.create_shader_module(wgpu::include_wgsl!("present.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("present"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        Present {
            pipeline,
            layout,
            uniforms: gpu.uniform_buffer::<Uniforms>("present uniforms"),
            format,
        }
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Draws `frame` centred on `view`, a `width` x `height` texture in
    /// [`Present::format`], at `scale` pixels per frame pixel, with alpha
    /// premultiplied; whatever the frame doesn't cover stays transparent
    pub fn draw(
        &self,
        gpu: &Gpu,
        frame: &OffscreenTarget,
        view: &wgpu::TextureView,
        (width, height): (u32, u32),
        scale: u32,
    ) {
        let device = gpu.device();
        let scale = scale.max(1);
        let origin = |screen: u32, frame: u32| (screen as i32 - (frame * scale) as i32) / 2;
        let uniforms = Uniforms {
            placement: [
                origin(width, frame.width()),
                origin(height, frame.height()),
                scale as i32,
                i32::from(self.format.is_srgb()),
            ],
        };
        gpu.queue()
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(frame.view()),
                },
            ],
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue().submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_pixels_match_the_stills() {
        let grid = |w, h| {
            let PixelGrid {
                width,
                height,
                scale,
            } = PixelGrid::for_canvas(w, h);
            (width, height, scale)
        };
        assert_eq!(grid(1920, 1080), (640, 360, 3));
        assert_eq!(grid(1080, 1920), (360, 640, 3));
        assert_eq!(grid(800, 450), (400, 225, 2));
        assert_eq!(grid(640, 360), (640, 360, 1));
        assert_eq!(grid(300, 200), (300, 200, 1));
        assert_eq!(grid(1281, 721), (427, 241, 3));
        assert_eq!(grid(0, 0), (1, 1, 1));
    }

    #[test]
    fn buffers_shrink_to_the_largest_texture() {
        assert_eq!(canvas_within(1920, 1080, 8192), (1920, 1080));
        assert_eq!(canvas_within(8192, 100, 8192), (8192, 100));
        assert_eq!(canvas_within(800, 33_554_432, 16384), (1, 16384));
        assert_eq!(canvas_within(20000, 10000, 16384), (16384, 8192));
        assert_eq!(canvas_within(3000, 60000, 2048), (102, 2048));
        for long in [16385, 40000, 1_000_000, u32::MAX] {
            for short in [0, 1, 333, 16384] {
                let (w, h) = canvas_within(long, short, 16384);
                assert_eq!(w, 16384);
                assert!((1..=16384).contains(&h));
                let (w, h) = canvas_within(short, long, 4096);
                assert_eq!(h, 4096);
                assert!((1..=4096).contains(&w));
            }
        }
    }

    #[test]
    fn long_side_never_passes_the_limit() {
        for long in 1..4000 {
            let grid = PixelGrid::for_canvas(long, long / 2);
            assert!(grid.width <= MAX_CANVAS_FRAME_PX, "{long}");
            assert!(grid.width * grid.scale >= long, "{long}");
            assert!((grid.width - 1) * grid.scale < long, "{long}");
        }
    }
}
