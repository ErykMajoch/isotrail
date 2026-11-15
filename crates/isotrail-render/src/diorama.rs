use std::collections::HashMap;

use glam::Vec3;
use isotrail_bundle::SceneBundle;

use crate::{
    Aim, Framing, Gpu, GpuMesh, Look, MAX_ZOOM_RANGE, OffscreenTarget, RenderError, RouteLine,
    Scene, SunState, TerrainMesh, TerrainRenderer, Turntable, View,
};

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
        let scene = Scene {
            mesh: &self.mesh,
            route: Some(&self.route),
            progress,
            star_seed: self.star_seed,
            time_s: self.time_s,
        };
        renderer.draw(gpu, target, &scene, &camera, &self.sun, look);
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
