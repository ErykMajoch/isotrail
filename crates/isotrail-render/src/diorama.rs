use std::collections::HashMap;

use glam::Vec3;
use isotrail_bundle::SceneBundle;

use crate::{
    Framing, Gpu, GpuMesh, Look, OffscreenTarget, RenderError, RouteLine, Scene, SunState,
    TerrainMesh, TerrainRenderer, Turntable, View, Viewpoints,
};

/// A bundle's terrain and route on the GPU, at one scale that fits the slab
/// from every heading it will be seen from, so that it can turn without
/// changing size
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
        viewpoints: &Viewpoints,
        sun: SunState,
    ) -> Result<Diorama, RenderError> {
        let outline = outline(&mesh, bundle.grid.cell_size_m as f32);
        let aim = &viewpoints.aim;
        let turntable = Turntable::new(
            aim.heading(bundle),
            aim.pitch_deg,
            aim.zoom,
            outline,
            framing,
            viewpoints.yaw_offsets_deg,
        );
        let metres_per_px = turntable.metres_per_px();
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

    pub fn turntable(&self) -> &Turntable {
        &self.turntable
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
