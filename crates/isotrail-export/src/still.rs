use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Camera, Frame, Framing, Gpu, Look, Mesher, OffscreenTarget, RouteLine, Scene, SunState,
    TerrainRenderer, view_heading,
};

use crate::ExportError;

/// Output pixels per internal pixel, until a style is settled
pub const DEFAULT_SCALE: u32 = 3;

/// Renders the bundle's diorama and its whole route at the framing's internal
/// resolution, looking `yaw_offset_deg` off the hike's heading, under the
/// fixed sun, finished with `look`
pub fn render_still(
    gpu: &Gpu,
    bundle: &SceneBundle,
    mesher: &dyn Mesher,
    framing: &Framing,
    yaw_offset_deg: f32,
    look: &Look,
) -> Result<Frame, ExportError> {
    let grid = &bundle.grid;
    let mesh = mesher.mesh(bundle);
    let heading = view_heading(bundle, yaw_offset_deg);
    let camera = Camera::diorama(heading, mesh.positions(), framing);
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
        },
        &camera,
        &SunState::late_morning(grid),
        look,
    );
    Ok(target.read_back(gpu)?)
}
