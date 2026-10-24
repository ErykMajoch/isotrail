use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Camera, Frame, Framing, Gpu, Look, Mesher, OffscreenTarget, SunState, TerrainRenderer,
    view_heading,
};

use crate::ExportError;

/// Output pixels per internal pixel, until a style is settled
pub const DEFAULT_SCALE: u32 = 3;

/// Renders the bundle's diorama at the framing's internal resolution, looking
/// `yaw_offset_deg` off the hike's heading, under the fixed sun, finished with
/// `look`
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
    let renderer = TerrainRenderer::new(gpu);
    let target = OffscreenTarget::new(gpu, framing.width, framing.height);
    renderer.draw(
        gpu,
        &target,
        &renderer.upload(gpu, &mesh),
        &camera,
        &SunState::late_morning(grid),
        look,
    );
    Ok(target.read_back(gpu)?)
}
