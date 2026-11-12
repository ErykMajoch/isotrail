use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Diorama, Frame, Framing, Gpu, Look, Mesher, Moment, Motion, OffscreenTarget, SunState,
    TerrainRenderer, View, Viewpoints, Walk, mesh_within,
};

use crate::ExportError;

/// Renders every frame of `motion` at the framing's internal resolution and
/// hands each to `each` with what it shows. The camera keeps one scale that
/// fits the diorama from every heading of the orbit, the sun follows the
/// clock at the route's head unless `motion` holds one, and frames that show
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
    let offsets: Vec<f32> = moments.iter().map(|m| m.yaw_offset_deg).collect();
    let renderer = TerrainRenderer::new(gpu);
    let start = SunState::for_hike(bundle);
    let mut diorama = Diorama::new(
        gpu,
        &renderer,
        bundle,
        mesh_within(mesher, bundle, gpu.max_buffer_size()).map_err(ExportError::from)?,
        framing,
        &Viewpoints {
            aim: motion.aim,
            yaw_offsets_deg: &offsets,
        },
        motion.sun.unwrap_or(start),
    )
    .map_err(ExportError::from)?;
    let target = OffscreenTarget::new(gpu, framing.width, framing.height);

    let mut shown: Option<(f32, Frame)> = None;
    for moment in &moments {
        let frame = match shown {
            Some((time_s, ref frame)) if time_s == moment.time_s => frame,
            _ => {
                if motion.sun.is_none() {
                    diorama.set_sun(
                        moment
                            .hike_time_s
                            .map_or(start, |time_s| SunState::during(bundle, time_s)),
                    );
                }
                diorama.set_time(moment.time_s);
                diorama.draw(
                    gpu,
                    &renderer,
                    &target,
                    &View::turned(moment.yaw_offset_deg),
                    moment.progress,
                    look,
                );
                let frame = target.read_back(gpu).map_err(ExportError::from)?;
                &shown.insert((moment.time_s, frame)).1
            }
        };
        each(moment, frame)?;
    }
    Ok(())
}
