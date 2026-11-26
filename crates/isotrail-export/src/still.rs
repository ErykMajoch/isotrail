use std::future::Future;

use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Aim, Camera, Frame, Framing, Gpu, GpuMesh, Look, OffscreenTarget, RouteLine, Scene, SunState,
    TerrainMesh, TerrainRenderer,
};
#[cfg(not(target_arch = "wasm32"))]
use isotrail_render::{Mesher, mesh_within};

use crate::ExportError;

/// A still drawn on the GPU, waiting to be read
pub struct DrawnStill {
    target: OffscreenTarget,
    /// Kept until the read, as the drawing used it
    _mesh: GpuMesh,
}

impl DrawnStill {
    /// Draws the bundle's diorama and its whole route at the framing's
    /// internal resolution, seen as `aim` sets, lit by `sun` and finished
    /// with `look`. `mesh` comes from the bundle's mesher without props
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        gpu: &Gpu,
        renderer: &TerrainRenderer,
        bundle: &SceneBundle,
        mut mesh: TerrainMesh,
        framing: &Framing,
        aim: &Aim,
        sun: &SunState,
        look: &Look,
    ) -> Result<DrawnStill, ExportError> {
        let camera = Camera::aimed(
            aim.heading(bundle),
            aim.pitch_deg,
            aim.zoom,
            mesh.positions(),
            framing,
        );
        mesh.add_props(bundle, camera.metres_per_px);
        // About one point per internal pixel
        let route = RouteLine::drape(bundle, &mesh.surface, camera.metres_per_px / 2.0);
        let target = OffscreenTarget::new(gpu, framing.width, framing.height);
        let gpu_mesh = renderer.upload(gpu, &mesh)?;
        renderer.draw(
            gpu,
            &target,
            &Scene {
                mesh: &gpu_mesh,
                route: Some(&route),
                progress: 1.0,
                star_seed: bundle.prop_seed as u32,
                time_s: 0.0,
            },
            &camera,
            sun,
            look,
        );
        Ok(DrawnStill {
            target,
            _mesh: gpu_mesh,
        })
    }

    /// Reads the still, blocking until the GPU has finished
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(&self, gpu: &Gpu) -> Result<Frame, ExportError> {
        Ok(self.target.read_back(gpu)?)
    }

    /// Reads the still, awaiting `pause` between checks
    pub async fn read_async<F: Future<Output = ()>>(
        &self,
        gpu: &Gpu,
        pause: impl FnMut() -> F,
    ) -> Result<Frame, ExportError> {
        Ok(self.target.read_back_async(gpu, pause).await?)
    }
}

/// Renders the bundle's diorama and its whole route at the framing's internal
/// resolution, seen as `aim` sets, lit by `sun` and finished with `look`
#[cfg(not(target_arch = "wasm32"))]
pub fn render_still(
    gpu: &Gpu,
    bundle: &SceneBundle,
    mesher: &dyn Mesher,
    framing: &Framing,
    aim: &Aim,
    sun: &SunState,
    look: &Look,
) -> Result<Frame, ExportError> {
    let mesh = mesh_within(mesher, bundle, gpu.max_buffer_size())?;
    let renderer = TerrainRenderer::new(gpu);
    DrawnStill::draw(gpu, &renderer, bundle, mesh, framing, aim, sun, look)?.read(gpu)
}
