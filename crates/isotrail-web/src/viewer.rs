use glam::Vec2;
use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Diorama, Framing, Gpu, Insets, Look, Mesher, OffscreenTarget, PixelGrid, Present, TerrainMesh,
    TerrainRenderer, View, VoxelMesher,
};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

const SAFE_AREA: Insets = Insets {
    top: 0.05,
    bottom: 0.05,
    left: 0.05,
    right: 0.05,
};
/// The diorama fits the canvas from every heading this far apart
const FIT_STEP_DEG: usize = 5;

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
    look: Look,
    stage: Option<Stage>,
    view: View,
    progress: f32,
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
    /// WebGL2 otherwise) or `"webgl"`. Nothing shows until `resize`
    pub async fn load(
        canvas: HtmlCanvasElement,
        bundle: Vec<u8>,
        backend: Option<String>,
    ) -> Result<Viewer, JsError> {
        console_error_panic_hook::set_once();
        let webgl_only = match backend.as_deref() {
            None | Some("" | "auto") => false,
            Some("webgl") => true,
            Some(other) => {
                return Err(JsError::new(&format!(
                    "unknown backend {other:?}: expected auto or webgl"
                )));
            }
        };
        let bundle = SceneBundle::from_bytes(&bundle)?;
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let (gpu, surface) = Gpu::for_canvas(canvas, webgl_only).await?;
        let capabilities = surface.get_capabilities(gpu.adapter());
        let mut config = surface
            .get_default_config(gpu.adapter(), width, height)
            .ok_or_else(|| JsError::new("the GPU can't draw into this canvas"))?;
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
        let mesh = VoxelMesher::default().mesh(&bundle);
        Ok(Viewer {
            gpu,
            surface,
            config,
            renderer,
            present,
            bundle,
            mesh,
            look: Look::default(),
            stage: None,
            view: View::default(),
            progress: 1.0,
        })
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

    /// Magnification, from 1 (the whole slab) to 3
    #[wasm_bindgen(getter)]
    pub fn zoom(&self) -> f32 {
        self.view.zoom
    }

    /// Sets the canvas's drawing buffer size in device pixels, and the pixel
    /// grid that fits it
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            self.stage = None;
            return;
        }
        (self.config.width, self.config.height) = (width, height);
        self.surface.configure(self.gpu.device(), &self.config);
        let grid = PixelGrid::for_canvas(width, height);
        if self.stage.as_ref().is_some_and(|stage| stage.grid == grid) {
            return;
        }
        let framing = Framing {
            width: grid.width,
            height: grid.height,
            safe: SAFE_AREA,
        };
        let turn: Vec<f32> = (0..360).step_by(FIT_STEP_DEG).map(|d| d as f32).collect();
        let diorama = Diorama::new(
            &self.gpu,
            &self.renderer,
            &self.bundle,
            self.mesh.clone(),
            &framing,
            &turn,
        );
        let frame = OffscreenTarget::new(&self.gpu, grid.width, grid.height);
        // At the new scale the focus can fall outside the whole-slab view
        self.view = diorama.turntable().clamp(self.view);
        self.stage = Some(Stage {
            grid,
            diorama,
            frame,
        });
    }

    /// Share of the route drawn, from 0 to 1
    #[wasm_bindgen(js_name = setProgress)]
    pub fn set_progress(&mut self, share: f32) {
        self.progress = share.clamp(0.0, 1.0);
    }

    /// Degrees the view turns from the still's
    #[wasm_bindgen(js_name = setYaw)]
    pub fn set_yaw(&mut self, degrees: f32) {
        self.view.yaw_offset_deg = degrees;
        if let Some(stage) = &self.stage {
            self.view = stage.diorama.turntable().clamp(self.view);
        }
    }

    /// Magnifies `factor` times, keeping what is under canvas pixel `x`, `y`
    /// in place
    #[wasm_bindgen(js_name = zoomBy)]
    pub fn zoom_by(&mut self, factor: f32, x: f32, y: f32) {
        if let Some(stage) = &self.stage {
            let pixel = self.frame_point(stage, x, y);
            self.view = stage.diorama.turntable().zoom_at(self.view, factor, pixel);
        }
    }

    /// Moves the view with a drag of `dx`, `dy` canvas pixels, once zoomed in
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if let Some(stage) = &self.stage {
            let pixels = Vec2::new(dx, dy) / stage.grid.scale as f32;
            self.view = stage.diorama.turntable().pan(self.view, pixels);
        }
    }

    /// Back to the whole slab from the still's heading
    #[wasm_bindgen(js_name = resetView)]
    pub fn reset_view(&mut self) {
        self.view = View::default();
    }

    /// Draws a frame into the canvas
    pub fn draw(&mut self) {
        let Some(stage) = &self.stage else {
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
        stage.diorama.draw(
            &self.gpu,
            &self.renderer,
            &stage.frame,
            &self.view,
            self.progress,
            &self.look,
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

impl Viewer {
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
