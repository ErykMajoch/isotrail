use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Diorama, Framing, Gpu, Look, Moment, Motion, OffscreenTarget, SunState, TerrainMesh,
    TerrainRenderer, View, Walk, When,
};
#[cfg(not(target_arch = "wasm32"))]
use isotrail_render::{Frame, Mesher, mesh_within};

use crate::ExportError;

/// The frames of a clip, drawn one at a time into the same target. Each
/// heading of the orbit shows the diorama as large as fits there, so the
/// clip settles on the still. The sun follows the clock at the route's head
/// unless the motion holds one
pub struct ClipFrames {
    motion: Motion,
    moments: Vec<Moment>,
    diorama: Diorama,
    target: OffscreenTarget,
    start: SunState,
    zoom: f32,
    next: usize,
    /// The moment the target shows
    drawn: Option<f32>,
}

impl ClipFrames {
    /// `mesh` comes from the bundle's mesher without props
    pub fn new(
        gpu: &Gpu,
        renderer: &TerrainRenderer,
        bundle: &SceneBundle,
        mesh: TerrainMesh,
        framing: &Framing,
        motion: &Motion,
    ) -> Result<ClipFrames, ExportError> {
        let walk = Walk::paced(bundle, motion.pacing);
        let moments = (0..motion.timeline.frame_count())
            .map(|i| motion.moment(&walk, i))
            .collect();
        let start = SunState::seen(bundle, When::Start, &motion.conditions);
        let sun = motion.sun.unwrap_or(start);
        let diorama = Diorama::new(gpu, renderer, bundle, mesh, framing, &motion.aim, sun)?;
        let zoom = diorama.start_view(&motion.aim).zoom;
        Ok(ClipFrames {
            motion: *motion,
            moments,
            diorama,
            target: OffscreenTarget::new(gpu, framing.width, framing.height),
            start,
            zoom,
            next: 0,
            drawn: None,
        })
    }

    pub fn len(&self) -> usize {
        self.moments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.moments.is_empty()
    }

    /// Moves on to the next frame and returns what it shows, or `None` after
    /// the last. The frame is drawn into the target only when it shows
    /// another moment than the one before; then the flag is true and the
    /// target needs reading again
    pub fn advance(
        &mut self,
        gpu: &Gpu,
        renderer: &TerrainRenderer,
        bundle: &SceneBundle,
        look: &Look,
    ) -> Option<(Moment, bool)> {
        let moment = *self.moments.get(self.next)?;
        self.next += 1;
        if self.drawn == Some(moment.time_s) {
            return Some((moment, false));
        }
        if self.motion.sun.is_none() {
            let conditions = &self.motion.conditions;
            self.diorama
                .set_sun(moment.hike_time_s.map_or(self.start, |time_s| {
                    SunState::seen(bundle, When::During(time_s), conditions)
                }));
        }
        self.diorama.set_time(moment.time_s);
        self.diorama.draw(
            gpu,
            renderer,
            &self.target,
            &View {
                zoom: self.zoom,
                ..View::turned(moment.yaw_offset_deg)
            },
            moment.progress,
            look,
        );
        self.drawn = Some(moment.time_s);
        Some((moment, true))
    }

    /// Where the frames are drawn
    pub fn target(&self) -> &OffscreenTarget {
        &self.target
    }
}

/// Renders every frame of `motion` at the framing's internal resolution and
/// hands each to `each` with what it shows. Frames that show the same moment
/// are rendered once
#[cfg(not(target_arch = "wasm32"))]
pub fn render_clip<E: From<ExportError>>(
    gpu: &Gpu,
    bundle: &SceneBundle,
    mesher: &dyn Mesher,
    framing: &Framing,
    look: &Look,
    motion: &Motion,
    mut each: impl FnMut(&Moment, &Frame) -> Result<(), E>,
) -> Result<(), E> {
    let renderer = TerrainRenderer::new(gpu);
    let mesh = mesh_within(mesher, bundle, gpu.max_buffer_size()).map_err(ExportError::from)?;
    let mut frames = ClipFrames::new(gpu, &renderer, bundle, mesh, framing, motion)?;
    let mut shown: Option<Frame> = None;
    while let Some((moment, drawn)) = frames.advance(gpu, &renderer, bundle, look) {
        if drawn || shown.is_none() {
            shown = Some(frames.target().read_back(gpu).map_err(ExportError::from)?);
        }
        if let Some(frame) = &shown {
            each(&moment, frame)?;
        }
    }
    Ok(())
}
