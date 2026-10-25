use glam::{IVec2, Vec3};
use wgpu::util::DeviceExt;

use crate::lighting::{BANDS, FILL};
use crate::post::{DEPTH_FORMAT, GBuffer, ID_FORMAT, NORMAL_FORMAT, PostPass, TERRAIN_ID};
use crate::route::{RoutePass, to_pixel};
use crate::sprite::Sprite;
use crate::{
    Camera, Gpu, Look, OffscreenTarget, RouteLine, SunState, TARGET_FORMAT, TerrainMesh, Vertex,
};

/// Texels along each side of the square shadow map, the most WebGL2 allows
pub const SHADOW_MAP_SIZE: u32 = 2048;
/// Pixels between flag poles when the start and finish meet
const FLAG_SPACING_PX: i32 = 8;

/// A terrain mesh uploaded to the GPU
pub struct GpuMesh {
    vertices: wgpu::Buffer,
    count: u32,
    corners: [Vec3; 8],
}

/// What a frame shows
pub struct Scene<'a> {
    pub mesh: &'a GpuMesh,
    pub route: Option<&'a RouteLine>,
    /// Share of the route drawn, by distance: 1 draws all of it with the
    /// finish marker, anything less ends in the head marker
    pub progress: f32,
}

impl<'a> Scene<'a> {
    pub fn terrain(mesh: &'a GpuMesh) -> Scene<'a> {
        Scene {
            mesh,
            route: None,
            progress: 1.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [f32; 16],
    light_view_proj: [f32; 16],
    sun: [f32; 4],
    sun_colour: [f32; 4],
    fill: [f32; 4],
    shadow: [f32; 4],
    object: [u32; 4],
}

/// Draws terrain meshes with banded Lambert lighting and sun shadows, and
/// the route over them
pub struct TerrainRenderer {
    pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    shadow_layout: wgpu::BindGroupLayout,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    route: RoutePass,
    post: PostPass,
}

impl TerrainRenderer {
    pub fn new(gpu: &Gpu) -> TerrainRenderer {
        let device = gpu.device();
        let module = device.create_shader_module(wgpu::include_wgsl!("terrain.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        // The shadow pass writes the shadow map, so it can't also bind it
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain shadow"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("terrain shadow"),
                bind_group_layouts: &[Some(&shadow_layout)],
                immediate_size: 0,
            });
        let vertex_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3,
                1 => Float32x3,
                2 => Unorm8x4,
            ],
        })];
        let depth_state = |bias| wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &vertex_buffers,
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(wgpu::DepthBiasState::default())),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[
                    Some(TARGET_FORMAT.into()),
                    Some(NORMAL_FORMAT.into()),
                    Some(ID_FORMAT.into()),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain shadow"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &vertex_buffers,
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(wgpu::DepthBiasState {
                constant: 2,
                slope_scale: 2.0,
                clamp: 0.0,
            })),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });

        let shadow_view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow map"),
                size: wgpu::Extent3d {
                    width: SHADOW_MAP_SIZE,
                    height: SHADOW_MAP_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        TerrainRenderer {
            pipeline,
            shadow_pipeline,
            layout,
            shadow_layout,
            shadow_view,
            shadow_sampler,
            route: RoutePass::new(gpu),
            post: PostPass::new(gpu),
        }
    }

    pub fn upload(&self, gpu: &Gpu, mesh: &TerrainMesh) -> GpuMesh {
        let vertices = gpu
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain vertices"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let (min, max) = mesh
            .positions()
            .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), p| {
                (min.min(p), max.max(p))
            });
        GpuMesh {
            vertices,
            count: mesh.vertices.len() as u32,
            corners: std::array::from_fn(|i| {
                Vec3::select(
                    glam::BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0),
                    max,
                    min,
                )
            }),
        }
    }

    /// Draws the scene into `target` over the sky, lit and shadowed by `sun`
    /// and finished with `look`
    pub fn draw(
        &self,
        gpu: &Gpu,
        target: &OffscreenTarget,
        scene: &Scene,
        camera: &Camera,
        sun: &SunState,
        look: &Look,
    ) {
        let device = gpu.device();
        let mesh = scene.mesh;
        let light_view_proj = if mesh.count == 0 {
            sun.shadow_view_proj([])
        } else {
            sun.shadow_view_proj(mesh.corners)
        };
        // Lookups move one shadow texel off the surface, so lit faces don't
        // shadow themselves
        let row_scale = |row: usize| light_view_proj.row(row).truncate().length();
        let texel_m = 2.0 / (row_scale(0).min(row_scale(1)) * SHADOW_MAP_SIZE as f32);
        let towards_camera = (-camera.forward * Vec3::new(1.0, 1.0, 0.0)).normalize_or_zero();
        let uniforms = Uniforms {
            view_proj: camera.view_proj.to_cols_array(),
            light_view_proj: light_view_proj.to_cols_array(),
            sun: sun.direction.normalize().extend(sun.ambient).to_array(),
            sun_colour: (sun.colour * sun.intensity).extend(BANDS as f32).to_array(),
            fill: towards_camera.extend(FILL).to_array(),
            shadow: [texel_m, 0.0, 0.0, 0.0],
            object: [TERRAIN_ID, 0, 0, 0],
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
            ],
        });
        let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain shadow"),
            layout: &self.shadow_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let gbuffer = GBuffer::new(gpu, target.width(), target.height());

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terrain shadow"),
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &shadow_bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.draw(0..mesh.count, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terrain"),
                color_attachments: &[&gbuffer.colour, &gbuffer.normal, &gbuffer.id].map(|view| {
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })
                }),
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &gbuffer.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.draw(0..mesh.count, 0..1);
        }
        let route = scene.route.filter(|line| !line.pieces().is_empty());
        let distance_m = route.map_or(0.0, |line| line.distance_at(scene.progress));
        self.route.encode(
            &mut encoder,
            &gbuffer,
            route,
            distance_m,
            camera,
            look.route.hidden_dash_px,
        );
        let mut sprites = Vec::new();
        if let (Some(line), Some(start)) = (route, route.and_then(RouteLine::start)) {
            let atlas = self.post.sprites();
            let pixel = |point: Vec3| {
                to_pixel(camera, point, target.width(), target.height())
                    .floor()
                    .as_ivec2()
            };
            let start = pixel(start.position);
            sprites.push(atlas.place(Sprite::Start, start));
            if scene.progress >= 1.0 {
                if let Some(finish) = line.finish() {
                    let mut finish = pixel(finish.position);
                    // Loops and out-and-backs fly both flags side by side
                    if (finish - start).abs().max_element() < FLAG_SPACING_PX {
                        finish = start + IVec2::new(FLAG_SPACING_PX, 0);
                    }
                    sprites.push(atlas.place(Sprite::Finish, finish));
                }
            } else if scene.progress > 0.0
                && let Some(head) = line.point_at(distance_m)
            {
                sprites.push(atlas.place(Sprite::Head, pixel(head)));
            }
        }
        self.post
            .encode(&mut encoder, &gbuffer, target, camera, look, &sprites);
        gpu.queue().submit([encoder.finish()]);
    }
}
