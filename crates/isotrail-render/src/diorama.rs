use std::collections::HashMap;
use std::ops::RangeInclusive;

use glam::Vec3;
use isotrail_bundle::SceneBundle;

use crate::{
    Aim, Background, Framing, Gpu, GpuMesh, Look, MAX_ZOOM_RANGE, OffscreenTarget, Placement,
    RenderError, RouteLine, Scene, SunState, TerrainMesh, TerrainRenderer, Turntable, View,
};

/// The two targets a turning diorama is drawn into
#[derive(Clone, Copy)]
pub struct Layers<'a> {
    /// Of the framing's size, for the sky
    pub backdrop: &'a OffscreenTarget,
    /// From [`Diorama::layer_target`], for the slab
    pub slab: &'a OffscreenTarget,
}

/// A bundle's terrain and route on the GPU, on a turntable. Its starting
/// view is the still with the same aim and framing
pub struct Diorama {
    mesh: GpuMesh,
    route: RouteLine,
    turntable: Turntable,
    sun: SunState,
    star_seed: u32,
    time_s: f32,
}

impl Diorama {
    /// `mesh` comes from the bundle's mesher without props
    pub fn new(
        gpu: &Gpu,
        renderer: &TerrainRenderer,
        bundle: &SceneBundle,
        mut mesh: TerrainMesh,
        framing: &Framing,
        aim: &Aim,
        sun: SunState,
    ) -> Result<Diorama, RenderError> {
        let turntable = Turntable::new(
            aim.heading(bundle),
            aim.pitch_deg,
            aim.fitted_zoom(),
            outline(&mesh),
            framing,
        );
        // Props and the route are sized for the starting view, as in the still
        let start = aim.start_view(*MAX_ZOOM_RANGE.end());
        let metres_per_px = turntable.camera(&start).metres_per_px;
        mesh.add_props(bundle, metres_per_px);
        // About one point per internal pixel
        let route = RouteLine::drape(bundle, &mesh.surface, metres_per_px / 2.0);
        Ok(Diorama {
            mesh: renderer.upload(gpu, &mesh)?,
            route,
            turntable,
            sun,
            star_seed: bundle.prop_seed as u32,
            time_s: 0.0,
        })
    }

    /// Where the view of a diorama made with `aim` starts, and returns to on
    /// reset
    pub fn start_view(&self, aim: &Aim) -> View {
        self.turntable
            .clamp(aim.start_view(self.turntable.max_zoom()))
    }

    pub fn turntable(&self) -> &Turntable {
        &self.turntable
    }

    pub fn set_max_zoom(&mut self, zoom: f32) {
        self.turntable.set_max_zoom(zoom);
    }

    /// Lights the frames drawn from now on with `sun`
    pub fn set_sun(&mut self, sun: SunState) {
        self.sun = sun;
    }

    /// Moves the clouds, rain and snow on to `time_s` seconds
    pub fn set_time(&mut self, time_s: f32) {
        self.time_s = time_s;
    }

    /// Draws the diorama as `view` shows it, with `progress` of the route
    /// walked, into a target of the framing's size
    pub fn draw(
        &self,
        gpu: &Gpu,
        renderer: &TerrainRenderer,
        target: &OffscreenTarget,
        view: &View,
        progress: f32,
        look: &Look,
    ) {
        let camera = self.turntable.camera(view);
        renderer.draw(gpu, target, &self.scene(progress), &camera, &self.sun, look);
    }

    /// Keeps room to turn within `sweep`, in degrees of yaw offset, without
    /// anything changing size, in a layer no larger than `max_side` pixels
    /// a side. See [`Turntable::make_room`]
    pub fn make_room(&mut self, sweep: RangeInclusive<f32>, max_side: u32) {
        self.turntable.make_room(sweep, max_side);
    }

    /// A target for [`Layers::slab`], if there is room to turn. Make one and
    /// reuse it
    pub fn layer_target(&self, gpu: &Gpu) -> Option<OffscreenTarget> {
        let margins = self.turntable.margins()?;
        let framing = self.turntable.framing();
        Some(OffscreenTarget::with_margins(
            gpu,
            framing.width,
            framing.height,
            margins,
        ))
    }

    /// Whether `view` is turned from the still's and there is room to draw
    /// it with [`Diorama::draw_turning`]
    pub fn turns(&self, view: &View) -> bool {
        view.yaw_offset_deg != 0.0 && self.turntable.margins().is_some()
    }

    /// Draws `view` at the still's pixel size: the slab into its layer and,
    /// under a sky, the sky into the backdrop. The layer goes over the
    /// backdrop where the returned placement says, which shows the slab as
    /// [`Diorama::draw`] frames it. `None`, with nothing drawn, when the
    /// layer has no room for this view
    pub fn draw_turning(
        &self,
        gpu: &Gpu,
        renderer: &TerrainRenderer,
        layers: Layers,
        view: &View,
        progress: f32,
        look: &Look,
    ) -> Option<Placement> {
        let turning = self.turntable.turning(view)?;
        let scene = self.scene(progress);
        if look.background == Background::Sky {
            let camera = &turning.backdrop;
            renderer.draw_sky(gpu, layers.backdrop, &scene, camera, &self.sun, look);
        }
        renderer.draw_slab(gpu, layers.slab, &scene, &turning.layer, &self.sun, look);
        Some(turning.placement)
    }

    fn scene(&self, progress: f32) -> Scene<'_> {
        Scene {
            mesh: &self.mesh,
            route: Some(&self.route),
            progress,
            star_seed: self.star_seed,
            time_s: self.time_s,
        }
    }
}

/// The highest and lowest vertex at each place on the ground. The camera
/// keeps up as up, so from any side these reach as far across the frame as
/// the whole mesh does, and frame it exactly as a still does
fn outline(mesh: &TerrainMesh) -> Vec<Vec3> {
    let mut cells: HashMap<(u32, u32), (Vec3, Vec3)> = HashMap::new();
    for p in mesh.positions() {
        let key = (p.x.to_bits(), p.y.to_bits());
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
