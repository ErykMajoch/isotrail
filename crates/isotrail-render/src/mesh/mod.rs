mod cover;
mod props;
mod season;
mod smooth;
mod strata;
mod surface;
mod voxel;

use glam::{Vec2, Vec3};
use isotrail_bundle::{GridInfo, Landcover, SceneBundle};
use season::Seasonal;

use crate::RenderError;

pub use season::foliage;
pub use smooth::SmoothMesher;
pub use surface::Surface;
pub use voxel::VoxelMesher;

/// How far the slab's base sits below the lowest terrain, in blocks of one
/// cell size
const BASE_BLOCKS: u32 = 6;
const SIDE_SHADE: f32 = 0.75;

/// Turns a bundle's terrain into triangles in the slab frame: x along the
/// long side, y across it (row 0 at +y), z up, all in metres from the slab
/// centre and the lowest terrain, with heights exaggerated
pub trait Mesher {
    fn mesh(&self, bundle: &SceneBundle) -> TerrainMesh;

    /// The most vertices [`Mesher::mesh`] and [`TerrainMesh::add_props`]
    /// would make together, without making them. Counting may stop at any
    /// number past `limit`
    fn vertex_bound(&self, bundle: &SceneBundle, limit: u64) -> u64;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub colour: [u8; 4],
}

/// Meshes the bundle, unless the mesh and its props could take more than
/// `max_bytes` in one buffer
pub fn mesh_within(
    mesher: &dyn Mesher,
    bundle: &SceneBundle,
    max_bytes: u64,
) -> Result<TerrainMesh, RenderError> {
    let max_vertices = max_bytes / VERTEX_BYTES;
    // Counting further keeps the size in the message close to the truth
    let limit = max_vertices.saturating_mul(4);
    let vertices = mesher.vertex_bound(bundle, limit);
    if vertices > max_vertices {
        let needed_mb = vertices.saturating_mul(VERTEX_BYTES).div_ceil(1 << 20);
        return Err(RenderError::MeshTooLarge {
            needed: if vertices > limit {
                format!("over {needed_mb} MB")
            } else {
                format!("up to {needed_mb} MB")
            },
            allowed_mb: max_bytes >> 20,
        });
    }
    Ok(mesher.mesh(bundle))
}

pub(crate) fn check_size(mesh: &TerrainMesh, max_bytes: u64) -> Result<(), RenderError> {
    let bytes = (mesh.vertices.len() as u64).saturating_mul(VERTEX_BYTES);
    if bytes > max_bytes {
        return Err(RenderError::MeshTooLarge {
            needed: format!("{} MB", bytes.div_ceil(1 << 20)),
            allowed_mb: max_bytes >> 20,
        });
    }
    Ok(())
}

const VERTEX_BYTES: u64 = size_of::<Vertex>() as u64;

/// Unindexed triangles, three vertices each, and the top they form
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerrainMesh {
    pub vertices: Vec<Vertex>,
    pub surface: Surface,
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

    fn push_quad(&mut self, corners: [Vec3; 4], colour: [u8; 4]) {
        let [a, b, c, d] = corners;
        self.push_triangle([a, b, c], colour);
        self.push_triangle([a, c, d], colour);
    }

    /// A vertical wall over `from` → `to`, facing `(to − from) × z`
    fn push_wall(&mut self, [from, to]: [Vec2; 2], bottom: f32, top: f32, colour: [u8; 4]) {
        self.push_quad(
            [
                from.extend(bottom),
                to.extend(bottom),
                to.extend(top),
                from.extend(top),
            ],
            colour,
        );
    }
}

/// A side of a cell, or of the whole slab
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// +y, towards row 0
    North,
    South,
    West,
    /// +x
    East,
}

impl Side {
    const ALL: [Side; 4] = [Side::North, Side::South, Side::West, Side::East];

    /// Grid corners of the cell's wall on this side, ordered so that
    /// [`TerrainMesh::push_wall`] faces it outwards
    fn wall(self, col: usize, row: usize) -> [(usize, usize); 2] {
        match self {
            Side::North => [(col + 1, row), (col, row)],
            Side::South => [(col, row + 1), (col + 1, row + 1)],
            Side::West => [(col, row), (col, row + 1)],
            Side::East => [(col + 1, row + 1), (col + 1, row)],
        }
    }

    fn neighbour(self, col: usize, row: usize, cols: usize, rows: usize) -> Option<(usize, usize)> {
        match self {
            Side::North => row.checked_sub(1).map(|r| (col, r)),
            Side::South => (row + 1 < rows).then_some((col, row + 1)),
            Side::West => col.checked_sub(1).map(|c| (c, row)),
            Side::East => (col + 1 < cols).then_some((col + 1, row)),
        }
    }

    /// The cells along this side of the slab, in order
    fn edge_cells(self, cols: usize, rows: usize) -> Vec<(usize, usize)> {
        match self {
            Side::North => (0..cols).map(|c| (c, 0)).collect(),
            Side::South => (0..cols).map(|c| (c, rows - 1)).collect(),
            Side::West => (0..rows).map(|r| (0, r)).collect(),
            Side::East => (0..rows).map(|r| (cols - 1, r)).collect(),
        }
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

/// The cells of the slab as walls and faces see them
struct Columns {
    cols: usize,
    rows: usize,
    /// Slab-frame x of the column boundaries, from -x to +x
    xs: Vec<f32>,
    /// Slab-frame y of the row boundaries, from +y to -y
    ys: Vec<f32>,
    landcover: Vec<u8>,
    /// Each cell's ground colour, which tops its cut-face column
    ground: Vec<[u8; 4]>,
    frozen: Vec<bool>,
    sea: Vec<bool>,
}

impl Columns {
    fn new(grid: &GridInfo, landcover: Vec<u8>) -> Columns {
        Columns {
            cols: grid.cols as usize,
            rows: grid.rows as usize,
            xs: (0..=grid.cols)
                .map(|col| slab_position(grid, col.into(), 0.0).x)
                .collect(),
            ys: (0..=grid.rows)
                .map(|row| slab_position(grid, 0.0, row.into()).y)
                .collect(),
            ground: landcover
                .iter()
                .map(|&class| landcover_rgba(class))
                .collect(),
            frozen: vec![false; landcover.len()],
            sea: vec![false; landcover.len()],
            landcover,
        }
    }

    /// The ground and lakes as the season has them
    fn in_season(mut self, bundle: &SceneBundle, seasonal: &Seasonal) -> Columns {
        self.ground = seasonal.ground(bundle);
        self.frozen = (0..self.landcover.len())
            .map(|cell| seasonal.frozen(cell))
            .collect();
        self.sea = (0..self.landcover.len())
            .map(|cell| seasonal.sea(cell))
            .collect();
        self
    }

    fn side(&self, cell: usize) -> [u8; 4] {
        shade(self.ground[cell])
    }

    fn corner(&self, (col, row): (usize, usize)) -> Vec2 {
        Vec2::new(self.xs[col], self.ys[row])
    }
}

fn landcover_rgba(value: u8) -> [u8; 4] {
    let [r, g, b] = Landcover::try_from(value).map_or([255, 0, 255], Landcover::legend_rgb);
    [r, g, b, 255]
}

fn shade([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    let shade = |c: u8| (f32::from(c) * SIDE_SHADE).round() as u8;
    [shade(r), shade(g), shade(b), a]
}

fn blend(from: [u8; 4], to: [u8; 4], share: f32) -> [u8; 4] {
    std::array::from_fn(|i| {
        (f32::from(from[i]) + (f32::from(to[i]) - f32::from(from[i])) * share).round() as u8
    })
}

fn hash<const N: usize>(seed: u64, keys: [u64; N]) -> u64 {
    keys.into_iter().fold(seed, |h, k| splitmix64(h ^ k))
}

fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use isotrail_testkit::{diorama_bundle, grid};

    use super::*;

    #[test]
    fn bounds_cover_the_mesh_and_its_props() {
        let bundle = diorama_bundle();
        let meshers: [&dyn Mesher; 3] = [
            &VoxelMesher::default(),
            &VoxelMesher { block_height: 0.25 },
            &SmoothMesher,
        ];
        for mesher in meshers {
            let mut mesh = mesher.mesh(&bundle);
            mesh.add_props(&bundle, 2.0);
            let bound = mesher.vertex_bound(&bundle, u64::MAX);
            let made = mesh.vertices.len() as u64;
            assert!(made <= bound, "{made} > {bound}");
            assert!(
                bound <= made + props::MAX_VERTICES * 2,
                "{bound} for {made}"
            );
        }
    }

    #[test]
    fn meshes_over_the_limit_are_refused() {
        let bundle = isotrail_testkit::bundle(
            grid(8, 4, 10.0),
            &[100.0; 32],
            &[isotrail_bundle::Landcover::Rock; 32],
        );
        let mesh = VoxelMesher::default().mesh(&bundle);
        let bytes = mesh.vertices.len() as u64 * VERTEX_BYTES;
        assert!(check_size(&mesh, bytes).is_ok());
        let message = check_size(&mesh, bytes - 1).unwrap_err().to_string();
        assert!(
            message.contains("needs 1 MB") && message.contains("(0 MB)"),
            "{message}"
        );

        let max = props::MAX_VERTICES * VERTEX_BYTES;
        let refused = mesh_within(&VoxelMesher::default(), &bundle, max).unwrap_err();
        assert!(matches!(refused, RenderError::MeshTooLarge { .. }));
        let room = max + bytes * 2;
        assert_eq!(
            mesh_within(&VoxelMesher::default(), &bundle, room).unwrap(),
            mesh
        );
    }
}
