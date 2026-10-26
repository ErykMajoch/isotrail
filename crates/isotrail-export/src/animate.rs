use std::collections::HashMap;

use glam::Vec3;
use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Camera, DEFAULT_YAW_OFFSET_DEG, Frame, Framing, Gpu, Look, Mesher, Moment, Motion,
    OffscreenTarget, RouteLine, Scene, SunState, TerrainMesh, TerrainRenderer, Walk, view_heading,
};

use crate::ExportError;

/// Renders every frame of `motion` at the framing's internal resolution and
/// hands each to `each` with what it shows. The camera keeps one scale that
/// fits the diorama from every heading of the orbit, and frames that show
/// the same moment are rendered once
pub fn render_clip<E: From<ExportError>>(
    gpu: &Gpu,
    bundle: &SceneBundle,
    mesher: &dyn Mesher,
    framing: &Framing,
    look: &Look,
    motion: &Motion,
    mut each: impl FnMut(&Moment, &Frame) -> Result<(), E>,
) -> Result<(), E> {
    let walk = Walk::new(bundle);
    let moments: Vec<Moment> = (0..motion.timeline.frame_count())
        .map(|i| motion.moment(&walk, i))
        .collect();
    let mut mesh = mesher.mesh(bundle);
    let outline = outline(&mesh, bundle.grid.cell_size_m as f32);
    let base = view_heading(bundle, DEFAULT_YAW_OFFSET_DEG);
    let heading = |moment: &Moment| base + moment.yaw_offset_deg.to_radians();
    let mut headings: Vec<f32> = moments.iter().map(heading).collect();
    headings.dedup();
    let scale = Camera::fit_scale(&headings, &outline, framing);

    mesh.add_props(bundle, scale);
    // About one point per internal pixel
    let route = RouteLine::drape(bundle, &mesh.surface, scale / 2.0);
    let renderer = TerrainRenderer::new(gpu);
    let target = OffscreenTarget::new(gpu, framing.width, framing.height);
    let gpu_mesh = renderer.upload(gpu, &mesh);
    let sun = SunState::late_morning(&bundle.grid);

    let mut shown: Option<(f32, Frame)> = None;
    for moment in &moments {
        let frame = match shown {
            Some((time_s, ref frame)) if time_s == moment.time_s => frame,
            _ => {
                let camera =
                    Camera::at_scale(heading(moment), outline.iter().copied(), framing, scale);
                let scene = Scene {
                    mesh: &gpu_mesh,
                    route: Some(&route),
                    progress: moment.progress,
                };
                renderer.draw(gpu, &target, &scene, &camera, &sun, look);
                let frame = target.read_back(gpu).map_err(ExportError::from)?;
                &shown.insert((moment.time_s, frame)).1
            }
        };
        each(moment, frame)?;
    }
    Ok(())
}

/// The highest and lowest point of the mesh over each cell, enough to frame
/// it from any side to within a cell
fn outline(mesh: &TerrainMesh, cell_m: f32) -> Vec<Vec3> {
    let mut cells: HashMap<(i32, i32), (Vec3, Vec3)> = HashMap::new();
    for p in mesh.positions() {
        let key = ((p.x / cell_m).floor() as i32, (p.y / cell_m).floor() as i32);
        cells
            .entry(key)
            .and_modify(|(low, high)| {
                if p.z < low.z {
                    *low = p;
                }
                if p.z > high.z {
                    *high = p;
                }
            })
            .or_insert((p, p));
    }
    cells
        .into_values()
        .flat_map(|(low, high)| [low, high])
        .collect()
}
