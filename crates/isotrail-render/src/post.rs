use std::f32::consts::PI;

use glam::{Vec2, Vec3, Vec4};

use crate::lighting::smoothstep;
use crate::palette::{MAX_COLOURS, srgb_to_oklab};
use crate::sprite::{Placed, SpriteAtlas};
use crate::weather::cloud_threshold;
use crate::{Camera, DEFAULT_PALETTE, Gpu, OffscreenTarget, Palette, SunState, TARGET_FORMAT};

/// The daytime sky's colour at the top and bottom of the frame
pub const SKY_TOP: [u8; 3] = [150, 190, 226];
pub const SKY_BOTTOM: [u8; 3] = [214, 230, 240];
pub const DEFAULT_OCCLUSION: f32 = 1.0;
pub const DEFAULT_SOFT_SHADOWS: f32 = 1.5;

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

    /// Every strength times `factor`, each kept within 0 and 1
    pub fn scaled(self, factor: f32) -> Edges {
        let scale = |strength: f32| (strength * factor).clamp(0.0, 1.0);
        Edges {
            silhouette: scale(self.silhouette),
            depth: scale(self.depth),
            crease: scale(self.crease),
            ..self
        }
    }
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
    /// In internal pixels, from 1 to 4
    pub width_px: u32,
    /// How much of the route's colour covers the terrain that hides it
    pub hidden_opacity: f32,
    /// Length of the hidden line's dashes and gaps, in internal pixels; 0
    /// draws it solid
    pub hidden_dash_px: f32,
    /// How much a one-pixel border darkens the terrain around the visible line
    pub outline: f32,
    pub markers: Markers,
}

impl Default for RouteStyle {
    fn default() -> Self {
        RouteStyle {
            colour: ROUTE_COLOUR,
            width_px: 2,
            hidden_opacity: 0.6,
            hidden_dash_px: 2.0,
            outline: 0.4,
            markers: Markers::ALL,
        }
    }
}

pub const ROUTE_COLOUR: [u8; 3] = [214, 72, 52];

impl RouteStyle {
    pub fn hiding(self, hidden: HiddenRoute) -> RouteStyle {
        let default = RouteStyle::default();
        let (hidden_opacity, hidden_dash_px) = match hidden {
            HiddenRoute::Dashed => (default.hidden_opacity, default.hidden_dash_px),
            HiddenRoute::Dim => (default.hidden_opacity, 0.0),
            HiddenRoute::Solid => (1.0, 0.0),
            HiddenRoute::Off => (0.0, 0.0),
        };
        RouteStyle {
            hidden_opacity,
            hidden_dash_px,
            ..self
        }
    }
}

/// How the parts of the route behind ridges show
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HiddenRoute {
    /// Faded and dashed
    #[default]
    Dashed,
    /// Faded
    Dim,
    /// Like the rest of the route
    Solid,
    Off,
}

/// The flags at the start and finish and the marker at the route's head
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Markers {
    pub start: bool,
    pub finish: bool,
    pub head: bool,
}

impl Markers {
    pub const ALL: Markers = Markers {
        start: true,
        finish: true,
        head: true,
    };
    pub const NONE: Markers = Markers {
        start: false,
        finish: false,
        head: false,
    };
}

impl Default for Markers {
    fn default() -> Markers {
        Markers::ALL
    }
}

/// What shows where there is no terrain
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Background {
    /// A dithered gradient that follows the sun, from [`SKY_TOP`] to
    /// [`SKY_BOTTOM`] by day
    #[default]
    Sky,
    Transparent,
    /// Kept exactly, outside the palette, so it can match a page
    Colour([u8; 3]),
}

/// The finishing passes: sky, outlines, route and palette
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// `None` keeps the lit colours as they are
    pub palette: Option<Palette>,
    pub background: Background,
    pub edges: Edges,
    /// How far ordered dithering moves the sky's colours before quantising,
    /// as a share of full brightness
    pub sky_dither: f32,
    pub route: RouteStyle,
    /// How much hollows, valley floors and the feet of slopes lose of the
    /// sky's light, from 0 (none) to 1
    pub occlusion: f32,
    /// How wide shadow edges soften, in cells; 0 keeps them hard
    pub soft_shadows: f32,
    pub god_rays: GodRays,
    /// Whether the terrain casts shadows
    pub shadows: bool,
    /// Stars and the moon's disc after dusk
    pub night_sky: bool,
}

/// Beams of sunlight slanting down through the scene
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GodRays {
    Off,
    /// When the sun breaks through a partly cloudy sky
    #[default]
    Weather,
    /// Whenever the sun is up
    On,
}

impl Look {
    /// No outlines and no palette, so the frame shows the lighting alone
    pub fn plain() -> Look {
        Look {
            palette: None,
            background: Background::Sky,
            edges: Edges::NONE,
            sky_dither: 0.0,
            route: RouteStyle::default(),
            occlusion: 0.0,
            soft_shadows: 0.0,
            god_rays: GodRays::Off,
            shadows: true,
            night_sky: true,
        }
    }
}

impl Default for Look {
    fn default() -> Self {
        Look {
            palette: Palette::builtin(DEFAULT_PALETTE),
            background: Background::Sky,
            edges: Edges::default(),
            sky_dither: 0.08,
            route: RouteStyle::default(),
            occlusion: DEFAULT_OCCLUSION,
            soft_shadows: DEFAULT_SOFT_SHADOWS,
            god_rays: GodRays::Weather,
            shadows: true,
            night_sky: true,
        }
    }
}

/// The sky behind the diorama as one frame shows it: a band of the sky half
/// a turn either side of the view spans the frame's width, and the horizon
/// sits a little above the middle with the zenith near the top
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sky {
    pub gradient: [[u8; 3]; 2],
    pub stars: f32,
    pub star_seed: u32,
    /// How far the stars have moved with the view's heading, in pixels
    pub star_shift_px: f32,
    pub moon: Option<MoonPixels>,
    pub weather: SkyWeather,
    pub rays: Rays,
}

/// Beams of sunlight breaking through gaps in the cloud: sheets of light
/// along the sun's direction, laid so that each pixel's view ray stays in
/// one sheet, which lines the beams in the sky up with the ground they light
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rays {
    /// How strongly they show with the sun up, from 0 to 1, and how much of
    /// that the weather allows
    pub sun: f32,
    pub weather: f32,
    /// What the frame draws, once the look has chosen
    pub strength: f32,
    /// Across the sheets, in the slab frame
    pub axis: Vec3,
    /// Which way the light travels across the frame, in pixels
    pub direction: Vec2,
    pub colour: Vec3,
    /// Width of a beam and its gap, and how far the wind has moved them
    pub spacing_m: f32,
    pub drift_m: f32,
}

impl Rays {
    /// Spaces the beams for a slab with these corners and sets how strongly
    /// `mode` shows them
    pub fn fit(&mut self, corners: [Vec3; 8], mode: GodRays) {
        let extent = (corners[7] - corners[0]).truncate().length();
        self.spacing_m = (extent / RAY_BEAMS).max(1.0);
        self.strength = match mode {
            GodRays::Off => 0.0,
            GodRays::Weather => self.sun * self.weather,
            GodRays::On => self.sun,
        };
    }
}

/// Clouds, haze, rain and snow as one frame shows them
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SkyWeather {
    /// The noise level clouds start at, above 1 for none
    pub cloud_threshold: f32,
    /// How far the wind has moved the clouds across the view, in pixels
    pub cloud_drift_px: f32,
    pub cloud_lit: Vec3,
    pub cloud_shade: Vec3,
    pub haze: f32,
    pub haze_colour: Vec3,
    /// How thickly each falls, from 0 to 1
    pub rain: f32,
    pub snow: f32,
    /// Sideways pixels for each pixel of fall
    pub rain_slant: f32,
    pub snow_slant: f32,
    /// Seconds into the clip
    pub time_s: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MoonPixels {
    pub centre: Vec2,
    pub radius: f32,
    /// Unit vector in the frame towards the disc's sunlit side
    pub lit_side: Vec2,
    /// Where the terminator crosses the disc's axis, from -1 at full moon to
    /// 1 at new moon
    pub terminator: f32,
    pub opacity: f32,
}

impl SkyWeather {
    const CLEAR: SkyWeather = SkyWeather {
        cloud_threshold: 2.0,
        cloud_drift_px: 0.0,
        cloud_lit: Vec3::ONE,
        cloud_shade: Vec3::ONE,
        haze: 0.0,
        haze_colour: Vec3::ONE,
        rain: 0.0,
        snow: 0.0,
        rain_slant: 0.0,
        snow_slant: 0.0,
        time_s: 0.0,
    };
}

/// Height of the horizon and of the zenith on the sky, as shares of the frame
const HORIZON: f32 = 0.45;
/// Beams and gaps across the slab's diagonal
const RAY_BEAMS: f32 = 12.0;
/// How much quicker than the wind the beams drift
const RAY_DRIFT: f32 = 20.0;
const ZENITH: f32 = 0.05;

impl Sky {
    pub fn new(
        camera: &Camera,
        sun: &SunState,
        star_seed: u32,
        time_s: f32,
        width: u32,
        height: u32,
    ) -> Sky {
        let ahead = (camera.forward * Vec3::new(1.0, 1.0, 0.0)).normalize_or(Vec3::Y);
        let right = ahead.cross(Vec3::Z);
        let (width, height) = (width as f32, height as f32);
        let px_per_rad = width / PI;
        let moon = sun.moon.map(|moon| {
            let m = moon.direction;
            let turn = m.dot(right).atan2(m.dot(ahead));
            let rise = HORIZON - (HORIZON - ZENITH) * m.z.clamp(-1.0, 1.0);
            let centre = Vec2::new(width / 2.0 + turn * px_per_rad, height * rise);
            // The sun's direction across the disc, as someone facing the moon sees it
            let across = m.cross(Vec3::Z).normalize_or(right);
            let up = across.cross(m);
            let lit_side = Vec2::new(moon.sun_direction.dot(across), -moon.sun_direction.dot(up))
                .normalize_or(Vec2::X);
            MoonPixels {
                centre: centre.floor() + 0.5,
                radius: (width.min(height) / 45.0).round().max(3.0),
                lit_side,
                terminator: 1.0 - 2.0 * moon.illuminated,
                opacity: moon.opacity,
            }
        });
        let weather = sun.weather.map_or(SkyWeather::CLEAR, |weather| {
            let light =
                sun.colour * sun.intensity * (1.0 - sun.ambient) + sun.ambient_colour * sun.ambient;
            // Mostly white, with a little of the light's colour at dawn and dusk
            let grey = light.dot(Vec3::new(0.2126, 0.7152, 0.0722));
            let cloud_lit = light.lerp(Vec3::splat(grey), 0.6).min(Vec3::ONE);
            let sky_top = Vec3::from_array(sun.sky[0].map(f32::from)) / 255.0;
            let sky_bottom = Vec3::from_array(sun.sky[1].map(f32::from)) / 255.0;
            let across = weather.wind.dot(right.truncate());
            let (rain, snow) = weather.rain_and_snow();
            let cover = weather.visible_cover();
            SkyWeather {
                cloud_threshold: if cover > 0.0 {
                    cloud_threshold(cover)
                } else {
                    SkyWeather::CLEAR.cloud_threshold
                },
                // Quicker than the real wind, so a clip shows them move
                cloud_drift_px: across * time_s * width / 200.0,
                cloud_lit,
                cloud_shade: (cloud_lit * 0.7).lerp(sky_top, 0.2),
                haze: weather.visible_haze(),
                haze_colour: sky_bottom.lerp(cloud_lit, 0.5),
                rain,
                snow,
                // Raindrops fall at about 6 m/s and snowflakes at about 1.5
                rain_slant: (across / 6.0).clamp(-0.8, 0.8),
                snow_slant: (across / 1.5).clamp(-1.5, 1.5),
                time_s,
            }
        });
        // Beams show best from a low sun, and the weather's share is the
        // sun breaking through gaps in the cloud
        let up = smoothstep(-0.02, 0.08, sun.direction.z) * (1.0 - sun.stars);
        let low = 1.0 - 0.5 * sun.direction.z.clamp(0.0, 1.0);
        let gaps = sun.weather.map_or(0.0, |weather| {
            let cover = weather.visible_cover();
            smoothstep(0.15, 0.35, cover) * smoothstep(0.95, 0.7, cover) * (1.0 - weather.dimming())
        });
        let light = (-sun.direction).normalize_or(Vec3::NEG_Z);
        let travel = camera.view_proj * light.extend(0.0);
        let direction = Vec2::new(travel.x * width, -travel.y * height).normalize_or(Vec2::Y);
        // The sheets hold both the light's direction and the view's, so a
        // view ray never crosses from one to the next
        let viewed = (camera.forward - camera.forward.dot(light) * light).normalize_or(right);
        let axis = light.cross(viewed).normalize_or(Vec3::X);
        let wind = sun.weather.map_or(Vec2::ZERO, |weather| weather.wind);
        let rays = Rays {
            sun: up * low,
            weather: gaps,
            strength: 0.0,
            axis,
            direction,
            colour: sun.colour.lerp(Vec3::ONE, 0.5).min(Vec3::ONE),
            spacing_m: 1.0,
            // Quicker than the real wind, like the clouds
            drift_m: wind.dot(axis.truncate()) * time_s * RAY_DRIFT,
        };
        Sky {
            gradient: sun.sky,
            stars: sun.stars,
            star_seed,
            star_shift_px: (ahead.y.atan2(ahead.x) * px_per_rad).round(),
            moon,
            weather,
            rays,
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
    night: [f32; 4],
    moon: [f32; 4],
    moon_phase: [f32; 4],
    weather: [f32; 4],
    falling: [f32; 4],
    cloud_lit: [f32; 4],
    cloud_shade: [f32; 4],
    haze_colour: [f32; 4],
    rays: [f32; 4],
    ray_axis: [f32; 4],
    ray_colour: [f32; 4],
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
    textures: Vec<wgpu::Texture>,
}

impl GBuffer {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> GBuffer {
        let mut textures = Vec::new();
        let mut view = |label, format| {
            let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
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
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            textures.push(texture);
            view
        };
        let (colour, normal, id, depth, route) = (
            view("g-buffer colour", TARGET_FORMAT),
            view("g-buffer normal", NORMAL_FORMAT),
            view("g-buffer id", ID_FORMAT),
            view("g-buffer depth", DEPTH_FORMAT),
            view("g-buffer route", ROUTE_FORMAT),
        );
        GBuffer {
            width,
            height,
            colour,
            normal,
            id,
            depth,
            route,
            textures,
        }
    }
}

impl Drop for GBuffer {
    fn drop(&mut self) {
        for texture in &self.textures {
            texture.destroy();
        }
    }
}

pub(crate) struct PostPass {
    device: wgpu::Device,
    queue: wgpu::Queue,
    uniforms: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    depth_sampler: wgpu::Sampler,
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
                texture(4, unfiltered),
                texture(5, wgpu::TextureSampleType::Uint),
                texture(6, unfiltered),
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
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
            queue: gpu.queue().clone(),
            uniforms: gpu.uniform_buffer::<Uniforms>("post uniforms"),
            pipeline,
            layout,
            depth_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("depth"),
                ..Default::default()
            }),
            sprites: SpriteAtlas::new(gpu),
        }
    }

    pub fn sprites(&self) -> &SpriteAtlas {
        &self.sprites
    }

    /// Finishes the frame over `sky` with `sprites` on top, the later ones in
    /// front
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &OffscreenTarget,
        camera: &Camera,
        sky: &Sky,
        look: &Look,
        sprites: &[Placed],
    ) {
        let device = &self.device;
        let gbuffer = target.gbuffer();
        let inv_view_proj = camera.view_proj.inverse();
        let metres_per_px = 2.0 * inv_view_proj.x_axis.truncate().length() / target.width() as f32;
        let colour =
            |rgb: [u8; 3]| Vec4::new(rgb[0].into(), rgb[1].into(), rgb[2].into(), 255.0) / 255.0;
        let ([sky_top, sky_bottom], background) = match look.background {
            Background::Sky => (sky.gradient, 0),
            Background::Colour(rgb) => ([rgb, rgb], 1),
            Background::Transparent => (sky.gradient, 2),
        };
        let weather = sky.weather;
        let moon = sky.moon.unwrap_or(MoonPixels {
            centre: Vec2::ZERO,
            radius: 0.0,
            lit_side: Vec2::X,
            terminator: 0.0,
            opacity: 0.0,
        });
        let mut uniforms = Uniforms {
            inv_view_proj: inv_view_proj.to_cols_array(),
            sky_top: colour(sky_top).to_array(),
            sky_bottom: colour(sky_bottom).to_array(),
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
                look.route.width_px.clamp(1, 4),
                sprites.len().min(MAX_SPRITES) as u32,
                background,
                sky.star_seed,
            ],
            night: [sky.stars, sky.star_shift_px, moon.opacity, moon.radius],
            moon: [
                moon.centre.x,
                moon.centre.y,
                moon.lit_side.x,
                moon.lit_side.y,
            ],
            moon_phase: [moon.terminator, 0.0, 0.0, 0.0],
            weather: [
                weather.cloud_threshold,
                weather.cloud_drift_px,
                weather.haze,
                weather.time_s,
            ],
            falling: [
                weather.rain,
                weather.snow,
                weather.rain_slant,
                weather.snow_slant,
            ],
            cloud_lit: weather.cloud_lit.extend(1.0).to_array(),
            cloud_shade: weather.cloud_shade.extend(1.0).to_array(),
            haze_colour: weather.haze_colour.extend(1.0).to_array(),
            rays: [
                sky.rays.strength,
                sky.rays.spacing_m,
                sky.rays.direction.x,
                sky.rays.direction.y,
            ],
            ray_axis: sky.rays.axis.extend(sky.rays.drift_m).to_array(),
            ray_colour: sky.rays.colour.extend(1.0).to_array(),
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
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
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
                    resource: self.uniforms.as_entire_binding(),
                },
                view(1, &gbuffer.colour),
                view(2, &gbuffer.normal),
                view(3, &gbuffer.id),
                view(4, &gbuffer.depth),
                view(5, &gbuffer.route),
                view(6, &self.sprites.view),
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&self.depth_sampler),
                },
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

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_6};

    use isotrail_testkit::grid;

    use super::*;
    use crate::{Aspect, MoonDisc};

    fn camera(heading: f32) -> Camera {
        let points = [
            Vec3::new(-100.0, -100.0, 0.0),
            Vec3::new(100.0, 100.0, 50.0),
        ];
        Camera::diorama(heading, points, &Aspect::Square.framing(6).unwrap())
    }

    fn sky(camera: &Camera, moon: Vec3, sun: Vec3, illuminated: f32) -> Sky {
        let night = SunState {
            moon: Some(MoonDisc {
                direction: moon.normalize(),
                sun_direction: sun.normalize(),
                illuminated,
                opacity: 1.0,
            }),
            ..SunState::at(&grid(10, 10, 10.0), 0.0, -20.0)
        };
        Sky::new(camera, &night, 7, 0.0, 180, 180)
    }

    #[test]
    fn beam_sheets_hold_the_light_and_the_view() {
        let view = camera(FRAC_PI_6);
        let slab = grid(10, 10, 10.0);
        let sun = SunState::at(&slab, 200.0, 15.0);
        let mut rays = Sky::new(&view, &sun, 7, 0.0, 180, 180).rays;
        // Every point on a view ray is in the same sheet
        assert!(rays.axis.dot(sun.direction).abs() < 1e-5);
        assert!(rays.axis.dot(view.forward).abs() < 1e-5);
        assert!(rays.sun > 0.5 && rays.weather == 0.0, "{rays:?}");

        let mut corners = [Vec3::new(-120.0, -50.0, -10.0); 8];
        corners[7] = Vec3::new(120.0, 50.0, 40.0);
        rays.fit(corners, GodRays::On);
        assert_eq!(rays.strength, rays.sun);
        assert_eq!(rays.spacing_m, 260.0 / RAY_BEAMS);
        for mode in [GodRays::Weather, GodRays::Off] {
            rays.fit(corners, mode);
            assert_eq!(rays.strength, 0.0, "{mode:?} under a clear sky");
        }
        let night = SunState::at(&slab, 200.0, -20.0);
        assert_eq!(Sky::new(&view, &night, 7, 0.0, 180, 180).rays.sun, 0.0);
    }

    #[test]
    fn the_moon_shows_where_it_stands_from_the_view() {
        let north = camera(FRAC_PI_2);
        let ahead = sky(&north, Vec3::Y, Vec3::X, 0.5).moon.unwrap();
        assert_eq!(ahead.centre, Vec2::new(90.5, 81.5));
        assert_eq!((ahead.radius, ahead.terminator), (4.0, 0.0));
        // Half a turn either side spans the frame, and the zenith is near the top
        let right = sky(&north, Vec3::new(1.0, 1.0, 0.0), Vec3::X, 0.5)
            .moon
            .unwrap();
        assert_eq!(right.centre, Vec2::new(135.5, 81.5));
        let overhead = sky(&north, Vec3::Z, Vec3::X, 0.5).moon.unwrap();
        assert_eq!(overhead.centre.y, 9.5);
    }

    #[test]
    fn the_sunlit_side_faces_the_sun() {
        let north = camera(FRAC_PI_2);
        let lit = |sun: Vec3| sky(&north, Vec3::Y, sun, 0.3).moon.unwrap().lit_side;
        assert!(lit(Vec3::X).abs_diff_eq(Vec2::X, 1e-5));
        assert!(lit(-Vec3::X).abs_diff_eq(-Vec2::X, 1e-5));
        assert!(lit(Vec3::new(0.0, 1.0, 1.0)).abs_diff_eq(-Vec2::Y, 1e-5));
        let full = sky(&north, Vec3::Y, -Vec3::Y, 1.0).moon.unwrap();
        assert_eq!(full.terminator, -1.0);
    }

    #[test]
    fn stars_and_moon_turn_together_with_the_view() {
        let (north, turned) = (camera(FRAC_PI_2), camera(FRAC_PI_2 - FRAC_PI_6));
        let (before, after) = (
            sky(&north, Vec3::Y, Vec3::X, 0.5),
            sky(&turned, Vec3::Y, Vec3::X, 0.5),
        );
        assert_eq!(before.star_shift_px - after.star_shift_px, 30.0);
        let moved = before.moon.unwrap().centre - after.moon.unwrap().centre;
        assert_eq!(moved, Vec2::new(30.0, 0.0));
        assert_eq!((before.stars, before.star_seed), (1.0, 7));
    }
}
