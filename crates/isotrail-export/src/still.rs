use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Camera, Frame, Framing, Gpu, Look, Mesher, OffscreenTarget, RouteLine, Scene, SunState,
    TerrainRenderer, view_heading,
};

use crate::ExportError;

/// Output pixels per internal pixel
pub const DEFAULT_SCALE: u32 = 3;

/// Renders the bundle's diorama and its whole route at the framing's internal
/// resolution, looking `yaw_offset_deg` off the hike's heading, lit by `sun`
/// and finished with `look`
pub fn render_still(
    gpu: &Gpu,
    bundle: &SceneBundle,
    mesher: &dyn Mesher,
    framing: &Framing,
    yaw_offset_deg: f32,
    sun: &SunState,
    look: &Look,
) -> Result<Frame, ExportError> {
    let mut mesh = mesher.mesh(bundle);
    let heading = view_heading(bundle, yaw_offset_deg);
    let camera = Camera::diorama(heading, mesh.positions(), framing);
    mesh.add_props(bundle, camera.metres_per_px);
    // About one point per internal pixel
    let route = RouteLine::drape(bundle, &mesh.surface, camera.metres_per_px / 2.0);
    let renderer = TerrainRenderer::new(gpu);
    let target = OffscreenTarget::new(gpu, framing.width, framing.height);
    let gpu_mesh = renderer.upload(gpu, &mesh);
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
