mod smooth;

use glam::{Vec2, Vec3};
use isotrail_bundle::{GridInfo, Landcover, SceneBundle};

pub use smooth::SmoothMesher;

/// Turns a bundle's terrain into triangles in the slab frame: x along the
/// long side, y across it (row 0 at +y), z up, all in metres from the slab
/// centre and the lowest terrain, with heights exaggerated
pub trait Mesher {
    fn mesh(&self, bundle: &SceneBundle) -> TerrainMesh;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub colour: [u8; 4],
}

/// Unindexed triangles, three vertices each
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerrainMesh {
    pub vertices: Vec<Vertex>,
}

impl TerrainMesh {
    pub fn triangles(&self) -> impl Iterator<Item = &[Vertex; 3]> {
        self.vertices.as_chunks::<3>().0.iter()
    }

    pub fn positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.vertices.iter().map(|v| Vec3::from(v.position))
    }

    /// Adds a triangle with its face normal, counter-clockwise seen from the
    /// side the normal points to
    fn push_triangle(&mut self, corners: [Vec3; 3], colour: [u8; 4]) {
        let [a, b, c] = corners;
        let normal = (b - a).cross(c - a).normalize_or(Vec3::Z).to_array();
        self.vertices.extend(corners.map(|p| Vertex {
            position: p.to_array(),
            normal,
            colour,
        }));
    }
}

/// Slab-frame position of a fractional grid position, where cell (c, r)
/// spans [c, c + 1) × [r, r + 1)
pub(crate) fn slab_position(grid: &GridInfo, col: f64, row: f64) -> Vec2 {
    Vec2::new(
        (col * grid.cell_size_m - grid.length_m() / 2.0) as f32,
        (grid.width_m() / 2.0 - row * grid.cell_size_m) as f32,
    )
}

fn landcover_rgba(value: u8) -> [u8; 4] {
    let [r, g, b] = Landcover::try_from(value).map_or([255, 0, 255], Landcover::legend_rgb);
    [r, g, b, 255]
}
