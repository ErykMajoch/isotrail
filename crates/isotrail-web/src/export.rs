use std::cell::RefCell;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::rc::Rc;

use isotrail_bundle::SceneBundle;
use isotrail_export::{ClipFrames, DrawnStill};
use isotrail_render::{
    Canvas, Frame, Framing, Gpu, Look, Motion, SunState, TerrainRenderer, Timeline, When,
};
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use crate::viewer::{Viewer, failure};

#[wasm_bindgen]
impl Viewer {
    /// Draws the still a `width` x `height` picture at scale 1 shows, with
    /// 5% margins and no text, in the current look and under the poster's
    /// light, ready to `read`. Fails with reason `"gpu"` when the GPU can't
    /// draw it
    pub fn still(&self, width: u32, height: u32) -> Result<StillRead, JsValue> {
        let framing = self.framing(width, height)?;
        let sun = self
            .options
            .held_sun
            .unwrap_or_else(|| SunState::seen(&self.bundle, When::Start, &self.options.conditions));
        let drawn = DrawnStill::draw(
            &self.gpu,
            &self.renderer,
            &self.bundle,
            self.mesh.clone(),
            &framing,
            &self.options.aim,
            &sun,
            &self.options.look,
        )
        .map_err(|err| failure("gpu", err))?;
        Ok(StillRead {
            gpu: self.gpu.clone(),
            drawn,
        })
    }

    /// The frames of an animation `duration` seconds long (10 by default) at
    /// `fps` frames a second (30 by default), framed as `still` frames them,
    /// in the current look, with the command line's other defaults. Fails
    /// with reason `"gpu"` when the GPU can't draw it
    pub fn clip(
        &self,
        width: u32,
        height: u32,
        duration: Option<f32>,
        fps: Option<u32>,
    ) -> Result<ClipRead, JsValue> {
        let framing = self.framing(width, height)?;
        let defaults = Timeline::default();
        let duration_s = duration.unwrap_or(defaults.duration_s);
        let fps = fps.unwrap_or(defaults.fps);
        if !(duration_s.is_finite() && duration_s > 0.0) || fps == 0 {
            return Err(JsError::new(&format!(
                "can't make an animation {duration_s}s long at {fps} frames a second"
            ))
            .into());
        }
        let options = &self.options;
        let motion = Motion {
            timeline: Timeline {
                duration_s,
                fps,
                ..defaults
            },
            pace: options.pace,
            sun: options.held_sun,
            conditions: options.conditions,
            aim: options.aim,
            pacing: options.pacing,
            ..Motion::default()
        };
        let frames = ClipFrames::new(
            &self.gpu,
            &self.renderer,
            &self.bundle,
            self.mesh.clone(),
            &framing,
            &motion,
        )
        .map_err(|err| failure("gpu", err))?;
        Ok(ClipRead {
            gpu: self.gpu.clone(),
            renderer: self.renderer.clone(),
            bundle: self.bundle.clone(),
            look: options.look.clone(),
            reading: RefCell::new(Some(Reading { frames, last: None })),
        })
    }
}

impl Viewer {
    fn framing(&self, width: u32, height: u32) -> Result<Framing, JsValue> {
        let largest = self.gpu.device().limits().max_texture_dimension_2d;
        if !(1..=largest).contains(&width) || !(1..=largest).contains(&height) {
            return Err(failure(
                "gpu",
                format!(
                    "the GPU can draw at most {largest} x {largest} pixels, not {width} x {height}"
                ),
            ));
        }
        Ok(Canvas::new(width, height)
            .framing(1)
            .expect("1 divides every size"))
    }
}

/// A still drawn on the GPU, waiting to be read
#[wasm_bindgen]
pub struct StillRead {
    gpu: Gpu,
    drawn: DrawnStill,
}

#[wasm_bindgen]
impl StillRead {
    /// The still's pixels, four bytes each (red, green, blue, alpha), row by
    /// row from the top. Fails with reason `"gpu"` when they can't be read
    pub async fn read(&self) -> Result<Uint8Array, JsValue> {
        let frame = self
            .drawn
            .read_async(&self.gpu, next_task)
            .await
            .map_err(|err| failure("gpu", err))?;
        Ok(Uint8Array::from(frame.rgba()))
    }
}

/// An animation's frames, read one at a time
#[wasm_bindgen]
pub struct ClipRead {
    gpu: Gpu,
    renderer: Rc<TerrainRenderer>,
    bundle: Rc<SceneBundle>,
    look: Look,
    /// Taken while a frame is read
    reading: RefCell<Option<Reading>>,
}

struct Reading {
    frames: ClipFrames,
    last: Option<Frame>,
}

// A panic aborts on wasm32, so no state is seen half changed
impl UnwindSafe for ClipRead {}
impl RefUnwindSafe for ClipRead {}

#[wasm_bindgen]
impl ClipRead {
    /// How many frames there are
    #[wasm_bindgen(getter)]
    pub fn frames(&self) -> usize {
        self.reading
            .borrow()
            .as_ref()
            .map_or(0, |reading| reading.frames.len())
    }

    /// The next frame's pixels as `StillRead.read` gives them, or undefined
    /// after the last. Wait for each before asking for the next. Fails with
    /// reason `"gpu"` when they can't be read
    pub async fn next(&self) -> Result<JsValue, JsValue> {
        let mut reading = self
            .reading
            .borrow_mut()
            .take()
            .ok_or_else(|| JsError::new("a frame is being read; wait for it first"))?;
        let read = self.next_frame(&mut reading).await;
        *self.reading.borrow_mut() = Some(reading);
        read
    }
}

impl ClipRead {
    async fn next_frame(&self, reading: &mut Reading) -> Result<JsValue, JsValue> {
        let Some((_, drawn)) =
            reading
                .frames
                .advance(&self.gpu, &self.renderer, &self.bundle, &self.look)
        else {
            return Ok(JsValue::UNDEFINED);
        };
        if drawn || reading.last.is_none() {
            let frame = reading
                .frames
                .target()
                .read_back_async(&self.gpu, next_task)
                .await
                .map_err(|err| failure("gpu", err))?;
            reading.last = Some(frame);
        }
        Ok(reading.last.as_ref().map_or(JsValue::UNDEFINED, |frame| {
            Uint8Array::from(frame.rgba()).into()
        }))
    }
}

/// Resolves in a later task, so the GPU's work can finish meanwhile. A
/// message isn't slowed down in a hidden tab, as a timer is
async fn next_task() {
    let promise = js_sys::Promise::new(&mut |resolve, _| match web_sys::MessageChannel::new() {
        Ok(channel) => {
            channel.port1().set_onmessage(Some(&resolve));
            let _ = channel.port2().post_message(&JsValue::UNDEFINED);
        }
        Err(_) => {
            let _ = resolve.call0(&JsValue::UNDEFINED);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
