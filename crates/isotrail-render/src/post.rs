use glam::Vec4;
use wgpu::util::DeviceExt;

use crate::palette::{MAX_COLOURS, srgb_to_oklab};
use crate::sprite::{Placed, SpriteAtlas};
use crate::{Camera, DEFAULT_PALETTE, Gpu, OffscreenTarget, Palette, TARGET_FORMAT};

/// The sky's colour at the top and bottom of the frame
pub const SKY_TOP: [u8; 3] = [150, 190, 226];
pub const SKY_BOTTOM: [u8; 3] = [214, 230, 240];

pub(crate) const NORMAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Uint;
pub(crate) const ROUTE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Uint;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Object ids in the geometry pass, 0 being the sky
pub(crate) const TERRAIN_ID: u32 = 1;

/// How strongly the outline pass marks edges, each strength a share of the
/// colour taken away or added
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edges {
    /// Where an object meets the sky or another object
    pub silhouette: f32,
    /// Where a surface passes in front of one further back
    pub depth: f32,
    /// Lightens outward folds such as ridges and the slab's rim
    pub crease: f32,
    /// How far, in internal pixels, a neighbour must lie off a surface to
    /// count as behind it
    pub depth_threshold_px: f32,
    /// How sharp a fold must be to count as a crease
    pub crease_angle_deg: f32,
}

impl Edges {
    pub const NONE: Edges = Edges {
        silhouette: 0.0,
        depth: 0.0,
        crease: 0.0,
        depth_threshold_px: 2.0,
        crease_angle_deg: 40.0,
    };
}

impl Default for Edges {
    fn default() -> Self {
        Edges {
            silhouette: 0.5,
            depth: 0.35,
            crease: 0.25,
            ..Edges::NONE
        }
    }
}

/// How the route is drawn over the terrain
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteStyle {
    pub colour: [u8; 3],
    /// In internal pixels, from 1 to 3
    pub width_px: u32,
    /// How much of the route's colour covers the terrain that hides it
    pub hidden_opacity: f32,
    /// Length of the hidden line's dashes and gaps, in internal pixels; 0
    /// draws it solid
    pub hidden_dash_px: f32,
    /// How much a one-pixel border darkens the terrain around the visible line
    pub outline: f32,
}

impl Default for RouteStyle {
    fn default() -> Self {
        RouteStyle {
            colour: [214, 72, 52],
            width_px: 2,
            hidden_opacity: 0.6,
            hidden_dash_px: 2.0,
            outline: 0.4,
        }
    }
}

/// The finishing passes: sky, outlines, route and palette
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// `None` keeps the lit colours as they are
    pub palette: Option<Palette>,
    pub edges: Edges,
    /// How far ordered dithering moves the sky's colours before quantising,
    /// as a share of full brightness
    pub sky_dither: f32,
    pub route: RouteStyle,
}

impl Look {
    /// No outlines and no palette, so the frame shows the lighting alone
    pub fn plain() -> Look {
        Look {
            palette: None,
            edges: Edges::NONE,
            sky_dither: 0.0,
            route: RouteStyle::default(),
        }
    }
}

impl Default for Look {
    fn default() -> Self {
        Look {
            palette: Palette::builtin(DEFAULT_PALETTE),
            edges: Edges::default(),
            sky_dither: 0.08,
            route: RouteStyle::default(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    inv_view_proj: [f32; 16],
    sky_top: [f32; 4],
    sky_bottom: [f32; 4],
    edges: [f32; 4],
    look: [f32; 4],
    route: [f32; 4],
    counts: [u32; 4],
    sprite_frame: [[i32; 4]; MAX_SPRITES],
    sprite_atlas: [[i32; 4]; MAX_SPRITES],
    palette_lab: [[f32; 4]; MAX_COLOURS],
    palette_rgb: [[f32; 4]; MAX_COLOURS],
}

/// The most sprites one frame can show
pub(crate) const MAX_SPRITES: usize = 4;

/// The geometry and route passes' targets at internal resolution
pub(crate) struct GBuffer {
    pub width: u32,
    pub height: u32,
    pub colour: wgpu::TextureView,
    pub normal: wgpu::TextureView,
    pub id: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    pub route: wgpu::TextureView,
}

impl GBuffer {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> GBuffer {
        let view = |label, format| {
            gpu.device()
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        GBuffer {
            width,
            height,
            colour: view("g-buffer colour", TARGET_FORMAT),
            normal: view("g-buffer normal", NORMAL_FORMAT),
            id: view("g-buffer id", ID_FORMAT),
            depth: view("g-buffer depth", DEPTH_FORMAT),
            route: view("g-buffer route", ROUTE_FORMAT),
        }
    }
}

pub(crate) struct PostPass {
    device: wgpu::Device,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sprites: SpriteAtlas,
}

impl PostPass {
    pub fn new(gpu: &Gpu) -> PostPass {
        let device = gpu.device();
        let module = device.create_shader_module(wgpu::include_wgsl!("post.wgsl"));
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let unfiltered = wgpu::TextureSampleType::Float { filterable: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
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
                texture(1, unfiltered),
                texture(2, unfiltered),
                texture(3, wgpu::TextureSampleType::Uint),
                texture(4, wgpu::TextureSampleType::Depth),
                texture(5, wgpu::TextureSampleType::Uint),
                texture(6, unfiltered),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post"),
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
                targets: &[Some(TARGET_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        PostPass {
            device: device.clone(),
            pipeline,
            layout,
            sprites: SpriteAtlas::new(gpu),
        }
    }

    pub fn sprites(&self) -> &SpriteAtlas {
        &self.sprites
    }

    /// Finishes the frame with `sprites` on top, the later ones in front
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        gbuffer: &GBuffer,
        target: &OffscreenTarget,
        camera: &Camera,
        look: &Look,
        sprites: &[Placed],
    ) {
        let device = &self.device;
        let inv_view_proj = camera.view_proj.inverse();
        let metres_per_px = 2.0 * inv_view_proj.x_axis.truncate().length() / target.width() as f32;
        let colour =
            |rgb: [u8; 3]| Vec4::new(rgb[0].into(), rgb[1].into(), rgb[2].into(), 255.0) / 255.0;
        let mut uniforms = Uniforms {
            inv_view_proj: inv_view_proj.to_cols_array(),
            sky_top: colour(SKY_TOP).to_array(),
            sky_bottom: colour(SKY_BOTTOM).to_array(),
            edges: [
                look.edges.silhouette,
                look.edges.depth,
                look.edges.crease,
                look.edges.depth_threshold_px * metres_per_px,
            ],
            look: [
                look.edges.crease_angle_deg.to_radians().cos(),
                look.sky_dither,
                look.palette.as_ref().map_or(0, |p| p.colours().len()) as f32,
                look.route.outline,
            ],
            route: colour(look.route.colour)
                .truncate()
                .extend(look.route.hidden_opacity)
                .to_array(),
            counts: [
                look.route.width_px.clamp(1, 3),
                sprites.len().min(MAX_SPRITES) as u32,
                0,
                0,
            ],
            sprite_frame: [[0; 4]; MAX_SPRITES],
            sprite_atlas: [[0; 4]; MAX_SPRITES],
            palette_lab: [[0.0; 4]; MAX_COLOURS],
            palette_rgb: [[0.0; 4]; MAX_COLOURS],
        };
        for (i, sprite) in sprites.iter().take(MAX_SPRITES).enumerate() {
            let (origin, size) = (sprite.origin, sprite.size);
            uniforms.sprite_frame[i] = [origin.x, origin.y, size.x, size.y];
            uniforms.sprite_atlas[i] = [sprite.atlas.x, sprite.atlas.y, 0, 0];
        }
        for (i, &rgb) in look.palette.iter().flat_map(|p| p.colours()).enumerate() {
            uniforms.palette_lab[i] = srgb_to_oklab(rgb).extend(0.0).to_array();
            uniforms.palette_rgb[i] = colour(rgb).to_array();
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("post uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                view(1, &gbuffer.colour),
                view(2, &gbuffer.normal),
                view(3, &gbuffer.id),
                view(4, &gbuffer.depth),
                view(5, &gbuffer.route),
                view(6, &self.sprites.view),
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
