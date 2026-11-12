//! The isotrail renderer: terrain meshers, camera, render passes and overlays
//! that turn a scene bundle into pixel-art frames, natively or in the browser
//!
//! This crate is wasm-safe: no filesystem, network or thread access

mod camera;
mod diorama;
mod error;
mod frame;
mod gpu;
mod labels;
mod lighting;
mod mesh;
mod occlusion;
mod options;
mod overlay;
mod palette;
mod post;
mod present;
mod route;
mod sprite;
mod target;
mod terrain;
mod test_scene;
mod text;
mod theme;
mod timeline;
mod weather;

pub use camera::{
    Aim, Aspect, Camera, Canvas, DEFAULT_YAW_OFFSET_DEG, Framing, Insets, MAX_ZOOM, MAX_ZOOM_RANGE,
    PITCH_DEG, Turntable, View, Viewpoints, route_heading, view_heading,
};
pub use diorama::Diorama;
pub use error::RenderError;
pub use frame::Frame;
pub use gpu::{ADAPTER_ENV, AdapterMode, Gpu};
pub use labels::{DATE_PLACEHOLDERS, Labels};
pub use lighting::{
    AMBIENT, BANDS, Conditions, FILL, Light, LightKey, MIN_LIGHT_ELEVATION_DEG, MOON_COLOUR,
    MOONLIGHT, Moon, MoonDisc, RAMP_ELEVATIONS_DEG, SUN_AZIMUTH_DEG, SUN_COLOUR, SUN_ELEVATION_DEG,
    SunState, When,
};
pub use mesh::{
    BASE_BLOCKS, Mesher, PathStyle, PropStyle, SeasonStyle, Shaped, SlabStyle, SmoothMesher,
    StrataStyle, Surface, Terrain, TerrainMesh, Vertex, VoxelMesher, WaterStyle, foliage,
    mesh_within,
};
pub use options::{
    BASE_BLOCKS_RANGE, BLOCK_HEIGHT_RANGE, HEADING_RANGE, MesherKind, OCCLUSION_RANGE,
    OUTLINES_RANGE, PITCH_RANGE, PROP_DENSITY_RANGE, ROUTE_WIDTH_RANGE, SOFT_SHADOWS_RANGE,
    SUN_AZIMUTH_RANGE, SUN_ELEVATION_RANGE, TURN_RANGE, ZOOM_RANGE, colour_name, parse_colour,
    parse_count, parse_sun, parse_switch, parse_within, switch_name,
};
pub use overlay::{
    DEFAULT_STATS, DateFormat, FitStep, Fitted, Layout, OverlayStyle, OverlayText, OverlayTheme,
    Overlays, OverlaysDontFit, PANEL_TINT, PanelStyle, SUBTEXT, Stat, StatKind, Subtitle, TEXT,
    TEXT_SHADOW, TitleSize, WEATHER_CREDIT, credit_line, format_date,
};
pub use palette::{DEFAULT_PALETTE, MAX_COLOURS, Palette, PaletteError, srgb_to_oklab};
pub use post::{
    Background, DEFAULT_OCCLUSION, DEFAULT_SOFT_SHADOWS, Edges, GodRays, HiddenRoute, Look,
    Markers, ROUTE_COLOUR, RouteStyle, SKY_BOTTOM, SKY_DITHER, SKY_TOP,
};
pub use present::{MAX_CANVAS_FRAME_PX, PixelGrid, Present, canvas_within};
pub use route::{RouteLine, RouteVertex};
pub use target::{OffscreenTarget, TARGET_FORMAT};
pub use terrain::{GpuMesh, SHADOW_MAP_SIZE, Scene, TerrainRenderer};
pub use test_scene::draw_test_scene;
pub use text::{PixelFont, TextEffect};
pub use theme::{Theme, ThemeError};
pub use timeline::{
    DEFAULT_DURATION_S, DEFAULT_FPS, DEFAULT_ORBIT_DEG, DRAW_SHARE, HOLD_SHARE, INTRO_SHARE,
    MAX_NIGHTS_SHARE, MIN_DRAW_S, Moment, Motion, NIGHT_SHARE, Orbit, PAUSE_S, Pace, Pacing,
    REWIND_SHARE, Step, Timeline, Walk,
};
pub use weather::{Atmosphere, ForcedWeather, WeatherEffects};
