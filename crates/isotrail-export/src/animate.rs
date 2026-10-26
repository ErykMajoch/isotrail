use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Diorama, Frame, Framing, Gpu, Look, Mesher, Moment, Motion, OffscreenTarget, TerrainRenderer,
    View, Walk,
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
    let offsets: Vec<f32> = moments.iter().map(|m| m.yaw_offset_deg).collect();
    let renderer = TerrainRenderer::new(gpu);
    let diorama = Diorama::new(
        gpu,
        &renderer,
        bundle,
        mesher.mesh(bundle),
        framing,
        &offsets,
    );
    let target = OffscreenTarget::new(gpu, framing.width, framing.height);

    let mut shown: Option<(f32, Frame)> = None;
    for moment in &moments {
        let frame = match shown {
            Some((time_s, ref frame)) if time_s == moment.time_s => frame,
            _ => {
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
