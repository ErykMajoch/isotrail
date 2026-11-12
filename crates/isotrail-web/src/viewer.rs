use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use glam::Vec2;
use isotrail_bundle::{BundleError, SceneBundle};
use isotrail_render::{
    Diorama, Framing, Gpu, Insets, OffscreenTarget, PixelGrid, Present, SunState, TerrainMesh,
    TerrainRenderer, View, Viewpoints, Walk, When, credit_line, mesh_within,
};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::options::Options;

const SAFE_AREA: Insets = Insets {
    top: 0.05,
    bottom: 0.05,
    left: 0.05,
    right: 0.05,
};
/// The diorama fits the canvas from every heading this far apart
const FIT_STEP_DEG: usize = 5;

// wgpu wants a device-lost callback it could send between threads, which a
// JavaScript function can't be, so the callback finds it by the viewer's id
thread_local! {
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
    static ON_LOST: RefCell<HashMap<u32, js_sys::Function>> = RefCell::new(HashMap::new());
}

/// A hike's diorama drawn into a canvas
#[wasm_bindgen]
pub struct Viewer {
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: TerrainRenderer,
    present: Present,
    bundle: SceneBundle,
    mesh: TerrainMesh,
    options: Options,
    stage: Option<Stage>,
    walk: Walk,
    view: View,
    progress: f32,
    /// The last share given to `setProgress`, none before the first
    share: Option<f32>,
    sun: SunState,
    time_s: f32,
    id: u32,
}

/// What a canvas size needs, rebuilt when the pixel grid changes
struct Stage {
    grid: PixelGrid,
    diorama: Diorama,
    frame: OffscreenTarget,
}

#[wasm_bindgen]
impl Viewer {
    /// Opens the GPU for `canvas` and loads a scene bundle (`.itb` bytes).
    /// `backend` is `"auto"` (the default: WebGPU where the browser has it,
    /// WebGL2 otherwise) or `"webgl"`. `options` sets the look, as
    /// `setOptions` does. Nothing shows until `resize`; a failure is an
    /// `Error` whose `reason` is `"version"` (a bundle from another
    /// isotrail), `"damaged"` (not a bundle, or cut short) or `"gpu"`
    pub async fn load(
        canvas: HtmlCanvasElement,
        bundle: Vec<u8>,
        backend: Option<String>,
        options: JsValue,
    ) -> Result<Viewer, JsValue> {
        console_error_panic_hook::set_once();
        let webgl_only = match backend.as_deref() {
            None | Some("" | "auto") => false,
            Some("webgl") => true,
            Some(other) => {
                return Err(JsError::new(&format!(
                    "unknown backend {other:?}: expected auto or webgl"
                ))
                .into());
            }
        };
        let bundle = SceneBundle::from_bytes(&bundle).map_err(|err| {
            let reason = match err {
                BundleError::OlderVersion { .. } | BundleError::NewerVersion { .. } => "version",
                _ => "damaged",
            };
            failure(reason, err)
        })?;
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let (gpu, surface) = Gpu::for_canvas(canvas, webgl_only)
            .await
            .map_err(|err| failure("gpu", err))?;
        let id = NEXT_ID.with(|next| next.replace(next.get().wrapping_add(1)));
        gpu.device()
            .set_device_lost_callback(move |reason, message| {
                if reason == wgpu::DeviceLostReason::Destroyed {
                    return;
                }
                let callback = ON_LOST.with_borrow(|callbacks| callbacks.get(&id).cloned());
                if let Some(callback) = callback {
                    let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&message));
                }
            });
        let capabilities = surface.get_capabilities(gpu.adapter());
        let mut config = surface
            .get_default_config(gpu.adapter(), width, height)
            .ok_or_else(|| failure("gpu", "the GPU can't draw into this canvas"))?;
        // Frames hold exact palette colours, so a format that doesn't
        // re-encode them is best
        if let Some(&format) = capabilities.formats.iter().find(|f| !f.is_srgb()) {
            config.format = format;
        }
        if capabilities
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
        }
        let renderer = TerrainRenderer::new(&gpu);
        let present = Present::new(&gpu, config.format);
        let options = Options::read(&options, &bundle);
        let mesh = mesh_within(options.mesher().as_ref(), &bundle, gpu.max_buffer_size())
            .map_err(|err| failure("gpu", err))?;
        let walk = Walk::paced(&bundle, options.pacing);
        let sun = options
            .held_sun
            .unwrap_or_else(|| SunState::seen(&bundle, When::Start, &options.conditions));
        let view = options.start_view();
        Ok(Viewer {
            gpu,
            surface,
            config,
            renderer,
            present,
            bundle,
            mesh,
            options,
            stage: None,
            walk,
            view,
            progress: 1.0,
            share: None,
            sun,
            time_s: 0.0,
            id,
        })
    }

    /// Calls `callback` with the browser's message if the GPU is lost, for
    /// instance when the driver resets. The viewer can't draw after that:
    /// free it and load the bundle again
    #[wasm_bindgen(js_name = onLost)]
    pub fn on_lost(&self, callback: js_sys::Function) {
        ON_LOST.with_borrow_mut(|callbacks| callbacks.insert(self.id, callback));
    }

    /// The graphics API in use, such as `"webgpu"` or `"gl"`
    #[wasm_bindgen(getter)]
    pub fn backend(&self) -> String {
        self.gpu.adapter_info().backend.to_string()
    }

    /// The hike's title from the bundle
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        self.bundle.meta.title.clone()
    }

    /// The data credits to show beside the diorama, joined with " · "
    #[wasm_bindgen(getter)]
    pub fn credit(&self) -> String {
        credit_line(&self.bundle).unwrap_or_default()
    }

    /// Magnification, from 1 (the whole slab) to `maxZoom`
    #[wasm_bindgen(getter)]
    pub fn zoom(&self) -> f32 {
        self.view.zoom
    }

    /// The largest magnification, 3 unless the options set another
    #[wasm_bindgen(getter, js_name = maxZoom)]
    pub fn max_zoom(&self) -> f32 {
        self.options.max_zoom
    }

    /// The route's colour as `#rrggbb`, from the options or the theme
    #[wasm_bindgen(getter, js_name = routeColour)]
    pub fn route_colour(&self) -> String {
        let [r, g, b] = self.options.look.route.colour;
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// Sets the canvas's drawing buffer size in device pixels, and the pixel
    /// grid that fits it; fails with reason `"gpu"` when the GPU can't hold
    /// the diorama at that size
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), JsValue> {
        if width == 0 || height == 0 {
            self.stage = None;
            return Ok(());
        }
        (self.config.width, self.config.height) = (width, height);
        self.surface.configure(self.gpu.device(), &self.config);
        let grid = PixelGrid::for_canvas(width, height);
        if self.stage.as_ref().is_some_and(|stage| stage.grid == grid) {
            return Ok(());
        }
        self.build_stage(grid)
    }

    /// Changes the look and the starting view: an object with any of
    /// `palette` (a built-in
    /// palette's name, `"none"`, or the text of a Lospec .hex file), `mesher`
    /// (`"voxel"` or `"smooth"`), `blockHeight`, `occlusion`, `softShadows`,
    /// `godRays` (`"auto"`, `"on"` or `"off"`), `background` (`"sky"`,
    /// `"transparent"` or `"#rrggbb"`) and `time` (`"HH:MM"`, local to the
    /// hike, which then holds the sun through playback); the scene's
    /// `propDensity` (0 to 2), `paths`, `streams`, `strata`, `shadows` and
    /// `nightSky` (`"on"` or `"off"`), `baseBlocks` (1 to 24), `outlines` (0
    /// to 2), `weatherEffects` (`"clouds,shade,rain,snow,haze"` or fewer, or
    /// `"none"`), `weather` (`"clear"`, `"cloudy"`, `"overcast"`, `"rain"`,
    /// `"snow"` or `"fog"`) and `sun` (`"AZIMUTH,ELEVATION"` in degrees,
    /// held like `time`); the route's `routeColour` (`"#rrggbb"`),
    /// `routeWidth` (1 to 4), `routeHidden` (`"dashed"`, `"dim"`, `"solid"`
    /// or `"off"`) and `markers` (`"start,finish,head"` or fewer, or
    /// `"none"`); `turn` (degrees, -360 to 360), `pitch` (degrees, 10 to
    /// 80) and `zoom` (0.5 to 2) for the view it starts from and resets to;
    /// `maxZoom` (1 to 6, default 3); `pace` (`"distance"` or `"time"`) for
    /// `setProgress`; and `theme`, the text of a theme file, under all of
    /// these. Values may be strings or numbers; one it can't use stays at
    /// its default with a console warning. Fails with reason `"gpu"` when
    /// the GPU can't hold the terrain the new options build
    #[wasm_bindgen(js_name = setOptions)]
    pub fn set_options(&mut self, options: JsValue) -> Result<(), JsValue> {
        let options = Options::read(&options, &self.bundle);
        let remesh = self.options.remeshes(&options);
        let refit = self.options.refits(&options);
        if options.pacing != self.options.pacing {
            self.walk = Walk::paced(&self.bundle, options.pacing);
        }
        self.options = options;
        if remesh {
            self.mesh = mesh_within(
                self.options.mesher().as_ref(),
                &self.bundle,
                self.gpu.max_buffer_size(),
            )
            .map_err(|err| failure("gpu", err))?;
        }
        if refit {
            self.view = self.options.start_view();
        }
        if let Some(stage) = &mut self.stage {
            stage.diorama.set_max_zoom(self.options.max_zoom);
            self.view = stage.diorama.turntable().clamp(self.view);
        }
        if (remesh || refit)
            && let Some(grid) = self.stage.take().map(|stage| stage.grid)
        {
            self.build_stage(grid)?;
        }
        if let Some(share) = self.share {
            self.progress = self.walk.step(self.options.pace, share).progress;
        }
        self.light();
        Ok(())
    }

    /// The diorama, its frame and turntable for a pixel grid
    fn build_stage(&mut self, grid: PixelGrid) -> Result<(), JsValue> {
        let framing = Framing {
            width: grid.width,
            height: grid.height,
            safe: SAFE_AREA,
        };
        let turn: Vec<f32> = (0..360).step_by(FIT_STEP_DEG).map(|d| d as f32).collect();
        self.stage = None;
        let mut diorama = Diorama::new(
            &self.gpu,
            &self.renderer,
            &self.bundle,
            self.mesh.clone(),
            &framing,
            &Viewpoints {
                aim: self.options.fitted_aim(),
                yaw_offsets_deg: &turn,
            },
            self.sun,
        )
        .map_err(|err| failure("gpu", err))?;
        diorama.set_max_zoom(self.options.max_zoom);
        let frame = OffscreenTarget::new(&self.gpu, grid.width, grid.height);
        // At the new scale the focus can fall outside the whole-slab view
        self.view = diorama.turntable().clamp(self.view);
        self.stage = Some(Stage {
            grid,
            diorama,
            frame,
        });
        Ok(())
    }

    /// How far through drawing the route, from 0 to 1, at an even speed (or
    /// following the recorded times, with `pace: "time"`) with a pause for
    /// each night, lit by the time at the route's head; until
    /// this is first called, the whole route shows under the sun at the
    /// start, as on the poster
    #[wasm_bindgen(js_name = setProgress)]
    pub fn set_progress(&mut self, share: f32) {
        if !share.is_finite() {
            return;
        }
        self.share = Some(share);
        self.progress = self.walk.step(self.options.pace, share).progress;
        self.light();
    }

    /// Moves the rain, snow and clouds on to `seconds`, any steadily rising
    /// clock
    #[wasm_bindgen(js_name = setTime)]
    pub fn set_time(&mut self, seconds: f32) {
        if !seconds.is_finite() {
            return;
        }
        self.time_s = seconds;
    }

    /// Whether rain or snow is falling under the current light, so frames
    /// keep changing
    #[wasm_bindgen(getter, js_name = weatherMoves)]
    pub fn weather_moves(&self) -> bool {
        self.sun.weather.is_some_and(|weather| weather.moves())
    }

    /// Degrees the view turns from the still's
    #[wasm_bindgen(js_name = setYaw)]
    pub fn set_yaw(&mut self, degrees: f32) {
        if !degrees.is_finite() {
            return;
        }
        self.view.yaw_offset_deg = degrees;
        if let Some(stage) = &self.stage {
            self.view = stage.diorama.turntable().clamp(self.view);
        }
    }

    /// Magnifies `factor` times, keeping what is under canvas pixel `x`, `y`
    /// in place
    #[wasm_bindgen(js_name = zoomBy)]
    pub fn zoom_by(&mut self, factor: f32, x: f32, y: f32) {
        if ![factor, x, y].iter().all(|n| n.is_finite()) {
            return;
        }
        if let Some(stage) = &self.stage {
            let pixel = self.frame_point(stage, x, y);
            self.view = stage.diorama.turntable().zoom_at(self.view, factor, pixel);
        }
    }

    /// Moves the view with a drag of `dx`, `dy` canvas pixels, once zoomed in
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        if let Some(stage) = &self.stage {
            let pixels = Vec2::new(dx, dy) / stage.grid.scale as f32;
            self.view = stage.diorama.turntable().pan(self.view, pixels);
        }
    }

    /// Back to the starting view
    #[wasm_bindgen(js_name = resetView)]
    pub fn reset_view(&mut self) {
        self.view = self.options.start_view();
        if let Some(stage) = &self.stage {
            self.view = stage.diorama.turntable().clamp(self.view);
        }
    }

    /// Draws a frame into the canvas
    pub fn draw(&mut self) {
        let Some(stage) = &mut self.stage else {
            return;
        };
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.gpu.device(), &self.config);
                return;
            }
            _ => return,
        };
        stage.diorama.set_time(self.time_s);
        stage.diorama.draw(
            &self.gpu,
            &self.renderer,
            &stage.frame,
            &self.view,
            self.progress,
            &self.options.look,
        );
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.present.draw(
            &self.gpu,
            &stage.frame,
            &view,
            (self.config.width, self.config.height),
            stage.grid.scale,
        );
        self.gpu.queue().present(texture);
    }
}

impl Drop for Viewer {
    /// Frees the GPU memory at once, rather than whenever the browser
    /// collects the objects that hold it
    fn drop(&mut self) {
        ON_LOST.with_borrow_mut(|callbacks| callbacks.remove(&self.id));
        self.gpu.device().destroy();
    }
}

/// An `Error` with a `reason` the element can tell apart
fn failure(reason: &str, err: impl std::fmt::Display) -> JsValue {
    let error = js_sys::Error::new(&err.to_string());
    let _ = js_sys::Reflect::set(&error, &"reason".into(), &reason.into());
    error.into()
}

impl Viewer {
    /// The sun held by the options, or the one at the route's head
    fn light(&mut self) {
        let step = self
            .share
            .map(|share| self.walk.step(self.options.pace, share));
        let conditions = &self.options.conditions;
        self.sun = match (self.options.held_sun, step.and_then(|step| step.time_s)) {
            (Some(held), _) => held,
            (None, Some(time_s)) => SunState::seen(&self.bundle, When::During(time_s), conditions),
            (None, None) => SunState::seen(&self.bundle, When::Start, conditions),
        };
        if let Some(stage) = &mut self.stage {
            stage.diorama.set_sun(self.sun);
        }
    }

    /// Canvas pixel to internal pixel, with the frame centred as `Present`
    /// draws it
    fn frame_point(&self, stage: &Stage, x: f32, y: f32) -> Vec2 {
        let grid = stage.grid;
        let origin = |canvas: u32, frame: u32| (canvas as f32 - (frame * grid.scale) as f32) / 2.0;
        Vec2::new(
            x - origin(self.config.width, grid.width),
            y - origin(self.config.height, grid.height),
        ) / grid.scale as f32
    }
}
