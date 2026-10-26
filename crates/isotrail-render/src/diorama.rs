use std::collections::HashMap;

use glam::Vec3;
use isotrail_bundle::SceneBundle;

use crate::{
    Camera, DEFAULT_YAW_OFFSET_DEG, Framing, Gpu, GpuMesh, Look, OffscreenTarget, RouteLine, Scene,
    SunState, TerrainMesh, TerrainRenderer, view_heading,
};

/// A bundle's terrain and route on the GPU, at one scale that fits the slab
/// from every heading it will be seen from, so that it can turn without
/// changing size
pub struct Diorama {
    mesh: GpuMesh,
    outline: Vec<Vec3>,
    route: RouteLine,
    framing: Framing,
    metres_per_px: f32,
    heading: f32,
    sun: SunState,
}

impl Diorama {
    /// `mesh` comes from the bundle's mesher without props, and
    /// `yaw_offsets_deg` are the turns from the still's view it will be drawn at
    pub fn new(
        gpu: &Gpu,
        renderer: &TerrainRenderer,
        bundle: &SceneBundle,
        mut mesh: TerrainMesh,
        framing: &Framing,
        yaw_offsets_deg: &[f32],
    ) -> Diorama {
        let outline = outline(&mesh, bundle.grid.cell_size_m as f32);
        let heading = view_heading(bundle, DEFAULT_YAW_OFFSET_DEG);
        let mut headings: Vec<f32> = yaw_offsets_deg
            .iter()
            .map(|offset| heading + offset.to_radians())
            .collect();
        headings.dedup();
        let metres_per_px = Camera::fit_scale(&headings, &outline, framing);
        mesh.add_props(bundle, metres_per_px);
        // About one point per internal pixel
        let route = RouteLine::drape(bundle, &mesh.surface, metres_per_px / 2.0);
        Diorama {
            mesh: renderer.upload(gpu, &mesh),
            outline,
            route,
            framing: *framing,
            metres_per_px,
            heading,
            sun: SunState::late_morning(&bundle.grid),
        }
    }

    /// Draws the diorama turned `yaw_offset_deg` from the still's view, with
    /// `progress` of the route walked, into a target of the framing's size
    pub fn draw(
        &self,
        gpu: &Gpu,
        renderer: &TerrainRenderer,
        target: &OffscreenTarget,
        yaw_offset_deg: f32,
        progress: f32,
        look: &Look,
    ) {
        let camera = Camera::at_scale(
            self.heading + yaw_offset_deg.to_radians(),
            self.outline.iter().copied(),
            &self.framing,
            self.metres_per_px,
        );
        let scene = Scene {
            mesh: &self.mesh,
            route: Some(&self.route),
            progress,
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
