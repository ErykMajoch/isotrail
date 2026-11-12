use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Aim, Camera, Frame, Framing, Gpu, Look, Mesher, OffscreenTarget, RouteLine, Scene, SunState,
    TerrainRenderer, mesh_within,
};

use crate::ExportError;

/// Output pixels per internal pixel
pub const DEFAULT_SCALE: u32 = 3;

/// Renders the bundle's diorama and its whole route at the framing's internal
/// resolution, seen as `aim` sets, lit by `sun` and finished with `look`
pub fn render_still(
    gpu: &Gpu,
    bundle: &SceneBundle,
    mesher: &dyn Mesher,
    framing: &Framing,
    aim: &Aim,
    sun: &SunState,
    look: &Look,
) -> Result<Frame, ExportError> {
    let mut mesh = mesh_within(mesher, bundle, gpu.max_buffer_size())?;
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
    let renderer = TerrainRenderer::new(gpu);
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
    Ok(target.read_back(gpu)?)
}
