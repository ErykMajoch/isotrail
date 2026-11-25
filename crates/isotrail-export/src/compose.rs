use isotrail_bundle::SceneBundle;
use isotrail_render::{
    Aim, Frame, Labels, Look, Mesher, Moment, OverlayOptions, Overlays, Palette, SunState, Walk,
};

#[cfg(not(target_arch = "wasm32"))]
use isotrail_render::{Gpu, Motion};

#[cfg(not(target_arch = "wasm32"))]
use crate::{ExportError, render_clip, render_still, upscale};

/// A still: the scene, how it is seen and lit, and the text over it
pub struct Still<'a> {
    pub bundle: &'a SceneBundle,
    pub mesher: &'a dyn Mesher,
    /// Laid out for the still's frame
    pub overlays: &'a Overlays,
    pub aim: &'a Aim,
    pub sun: &'a SunState,
    pub look: &'a Look,
}

impl Still<'_> {
    /// The finished still: the diorama framed clear of the overlays, the
    /// overlays over it, and every pixel enlarged `scale` times
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render(&self, gpu: &Gpu, scale: u32) -> Result<Frame, ExportError> {
        let mut frame = render_still(
            gpu,
            self.bundle,
            self.mesher,
            &self.overlays.diorama_framing(),
            self.aim,
            self.sun,
            self.look,
        )?;
        self.overlays
            .draw(&mut frame, self.look.palette.as_ref(), 1.0);
        upscale(&frame, scale)
    }
}

/// The text over each frame of a clip, with the stats counting up
pub struct Caption<'a> {
    pub bundle: &'a SceneBundle,
    /// Laid out for the clip's frame with the final stats
    pub overlays: &'a Overlays,
    pub options: &'a OverlayOptions,
    pub labels: &'a Labels,
}

impl Caption<'_> {
    /// Draws the overlays over a frame of the clip at `moment`, with the
    /// stats walked so far and the profile walked as far as the route
    pub fn finish(
        &self,
        walk: &Walk,
        moment: &Moment,
        frame: &mut Frame,
        palette: Option<&Palette>,
    ) {
        let walked = walk.stats_at(moment);
        let stats = self.options.walked_stats(self.bundle, &walked, self.labels);
        self.overlays
            .with_stats(&stats)
            .draw(frame, palette, moment.progress);
    }
}

/// Renders every frame of `motion` with `caption` over it, at the internal
/// size, hands each to `each`, and returns how many there were. The writers
/// enlarge them
#[cfg(not(target_arch = "wasm32"))]
pub fn finished_clip<E: From<ExportError>>(
    gpu: &Gpu,
    mesher: &dyn Mesher,
    look: &Look,
    motion: &Motion,
    caption: &Caption,
    mut each: impl FnMut(&Moment, &Frame) -> Result<(), E>,
) -> Result<u32, E> {
    let walk = Walk::paced(caption.bundle, motion.pacing);
    let palette = look.palette.as_ref();
    let mut frames = 0;
    render_clip(
        gpu,
        caption.bundle,
        mesher,
        &caption.overlays.diorama_framing(),
        look,
        motion,
        |moment, frame| -> Result<(), E> {
            let mut frame = frame.clone();
            caption.finish(&walk, moment, &mut frame, palette);
            each(moment, &frame)?;
            frames += 1;
            Ok(())
        },
    )?;
    Ok(frames)
}
