use std::collections::HashMap;

use glam::{Vec2, Vec3};
use isotrail_bundle::{Landcover, SceneBundle};
use tracing::debug;

use super::{TerrainMesh, hash, slab_position};

/// Internal pixels between neighbouring spots where a prop may stand
const SPACING_PX: f32 = 4.0;
/// The most spots a slab gets, whatever the pixel size
const MAX_SPOTS: usize = 10_000;
/// How far a prop may stray from its spot, as a share of the spacing
const JITTER: f32 = 0.35;
/// Internal pixels kept free of props on either side of the route
const CLEARING_PX: f32 = 2.0;
/// How much props vary in height either way
const HEIGHT_JITTER: f32 = 0.2;
const PROP_KEY: u64 = 2 << 32;

const NEEDLES: [u8; 4] = [40, 94, 58, 255];
const LEAVES: [u8; 4] = [84, 148, 62, 255];
const BUSH: [u8; 4] = [108, 138, 58, 255];
const TRUNK: [u8; 4] = [98, 68, 46, 255];
const BOULDER: [u8; 4] = [152, 148, 140, 255];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prop {
    Conifer,
    Broadleaf,
    Bush,
    Boulder,
}

/// A box of a prop in internal pixels: its half-width and its bottom and
/// top above the ground
struct Block(f32, f32, f32, [u8; 4]);

impl Prop {
    /// What stands on a spot of `class` ground, from two seeded rolls in [0, 1)
    fn on(class: Landcover, roll: f32, pick: f32) -> Option<Prop> {
        let (share, prop) = match class {
            Landcover::Coniferous => (0.8, Prop::Conifer),
            Landcover::Broadleaf => (0.7, Prop::Broadleaf),
            Landcover::MixedForest if pick < 0.5 => (0.75, Prop::Conifer),
            Landcover::MixedForest => (0.75, Prop::Broadleaf),
            Landcover::Scrub => (0.25, Prop::Bush),
            Landcover::Scree => (0.15, Prop::Boulder),
            _ => return None,
        };
        (roll < share).then_some(prop)
    }

    fn blocks(self) -> &'static [Block] {
        match self {
            Prop::Conifer => &[
                Block(0.5, 0.0, 1.0, TRUNK),
                Block(1.5, 1.0, 2.5, NEEDLES),
                Block(1.0, 2.5, 4.0, NEEDLES),
                Block(0.5, 4.0, 5.0, NEEDLES),
            ],
            Prop::Broadleaf => &[Block(0.5, 0.0, 1.5, TRUNK), Block(1.5, 1.5, 4.0, LEAVES)],
            Prop::Bush => &[Block(1.0, 0.0, 1.5, BUSH)],
            Prop::Boulder => &[Block(0.75, 0.0, 1.0, BOULDER)],
        }
    }

    fn half_width(self) -> f32 {
        self.blocks().iter().map(|b| b.0).fold(0.0, f32::max)
    }
}

impl TerrainMesh {
    /// Scatters trees over forest and scrub and boulders over scree, sized
    /// in internal pixels of `metres_per_px` and seeded by the bundle, clear
    /// of the route and the line mask; returns how many it placed
    pub fn add_props(&mut self, bundle: &SceneBundle, metres_per_px: f32) -> usize {
        let grid = &bundle.grid;
        let (cols, rows) = (grid.cols as usize, grid.rows as usize);
        let cell_m = grid.cell_size_m as f32;
        let sized = metres_per_px > 0.0 && cell_m > 0.0;
        if cols == 0 || rows == 0 || !sized {
            return 0;
        }
        let px = metres_per_px / cell_m;
        let mut spacing =
            (SPACING_PX * px).max((cols as f32 * rows as f32 / MAX_SPOTS as f32).sqrt());
        let spots = |spacing: f32| {
            let along = |n: usize| (n as f32 / spacing).ceil() as usize;
            (along(cols), along(rows))
        };
        while spots(spacing).0 * spots(spacing).1 > MAX_SPOTS {
            spacing *= 1.05;
        }
        let (across, down) = spots(spacing);
        let widest = Prop::Conifer.half_width();
        let route = RouteClearance::new(bundle, (CLEARING_PX + widest) * px);

        let seed = bundle.prop_seed;
        let unit = |i: usize, j: usize, k: u64| {
            (hash(seed, [PROP_KEY, i as u64, j as u64, k]) >> 40) as f32 / (1u64 << 24) as f32
        };
        let mut placed = 0;
        for j in 0..down {
            for i in 0..across {
                let centre = Vec2::new(
                    (i as f32 + 0.5 + (unit(i, j, 0) * 2.0 - 1.0) * JITTER) * spacing,
                    (j as f32 + 0.5 + (unit(i, j, 1) * 2.0 - 1.0) * JITTER) * spacing,
                );
                let (col, row) = (centre.x as usize, centre.y as usize);
                if col >= cols || row >= rows {
                    continue;
                }
                let cell = row * cols + col;
                if bundle
                    .terrain
                    .lines
                    .get(cell)
                    .is_some_and(|&bits| bits != 0)
                {
                    continue;
                }
                let Ok(class) = Landcover::try_from(bundle.terrain.landcover[cell]) else {
                    continue;
                };
                let Some(prop) = Prop::on(class, unit(i, j, 2), unit(i, j, 3)) else {
                    continue;
                };
                let half = prop.half_width() * px;
                let inside = centre.x >= half
                    && centre.y >= half
                    && centre.x + half <= cols as f32
                    && centre.y + half <= rows as f32;
                if !inside || route.near(centre, (CLEARING_PX + prop.half_width()) * px) {
                    continue;
                }
                // Props stand at full height on the ground under their
                // centre, with their foot reaching down to the lowest ground
                // they cover so slopes and steps never leave them floating
                let ground = self.surface.height(centre, centre);
                let lowest = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
                    .map(|(x, y)| {
                        let at = centre + Vec2::new(x, y) * half * 0.999;
                        self.surface.height(at, at)
                    })
                    .into_iter()
                    .fold(ground, f32::min);
                let at = slab_position(grid, centre.x.into(), centre.y.into());
                let height = metres_per_px * (1.0 + (unit(i, j, 4) * 2.0 - 1.0) * HEIGHT_JITTER);
                for &Block(half, bottom, top, colour) in prop.blocks() {
                    let half = Vec2::splat(half * metres_per_px);
                    let foot = if bottom == 0.0 {
                        lowest
                    } else {
                        ground + bottom * height
                    };
                    self.push_box(
                        (at - half).extend(foot),
                        (at + half).extend(ground + top * height),
                        colour,
                    );
                }
                placed += 1;
            }
        }
        debug!(props = placed, spacing_m = spacing * cell_m, "props");
        placed
    }

    /// An axis-aligned box without its bottom face
    fn push_box(&mut self, min: Vec3, max: Vec3, colour: [u8; 4]) {
        let corner = |x: f32, y: f32| Vec2::new(x, y);
        let (sw, se) = (corner(min.x, min.y), corner(max.x, min.y));
        let (ne, nw) = (corner(max.x, max.y), corner(min.x, max.y));
        self.push_quad([sw, se, ne, nw].map(|p| p.extend(max.z)), colour);
        for wall in [[sw, se], [se, ne], [ne, nw], [nw, sw]] {
            self.push_wall(wall, min.z, max.z, colour);
        }
    }
}

/// The route's segments, in grid positions, bucketed for nearby lookups
struct RouteClearance {
    segments: Vec<[Vec2; 2]>,
    buckets: HashMap<(i32, i32), Vec<usize>>,
    size: f32,
}

impl RouteClearance {
    /// Buckets sized for lookups up to `reach` cells away
    fn new(bundle: &SceneBundle, reach: f32) -> RouteClearance {
        let route = &bundle.route;
        let points: Vec<Vec2> = route
            .points
            .iter()
            .map(|p| Vec2::new(p.col, p.row))
            .collect();
        let mut segments: Vec<[Vec2; 2]> = (1..points.len())
            .filter(|&i| !route.breaks.contains(&(i as u32)))
            .map(|i| [points[i - 1], points[i]])
            .collect();
        // A piece of one point still clears its spot
        for (i, &point) in points.iter().enumerate() {
            let alone = |j: usize| j == 0 || route.breaks.contains(&(j as u32));
            if alone(i) && (i + 1 == points.len() || alone(i + 1)) {
                segments.push([point, point]);
            }
        }
        let size = reach.max(1.0);
        let mut buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (index, &[a, b]) in segments.iter().enumerate() {
            let key = |p: Vec2| ((p.x / size).floor() as i32, (p.y / size).floor() as i32);
            let (lo, hi) = (key(a.min(b) - reach), key(a.max(b) + reach));
            for y in lo.1..=hi.1 {
                for x in lo.0..=hi.0 {
                    buckets.entry((x, y)).or_default().push(index);
                }
            }
        }
        RouteClearance {
            segments,
            buckets,
            size,
        }
    }

    /// Whether the route passes within `reach` cells of `p`, for a `reach`
    /// up to the one the buckets were made for
    fn near(&self, p: Vec2, reach: f32) -> bool {
        let key = (
            (p.x / self.size).floor() as i32,
            (p.y / self.size).floor() as i32,
        );
        self.buckets.get(&key).is_some_and(|indices| {
            indices.iter().any(|&i| {
                let [a, b] = self.segments[i];
                let ab = b - a;
                let t = if ab == Vec2::ZERO {
                    0.0
                } else {
                    ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
                };
                p.distance(a + ab * t) < reach
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::{RoutePoint, line_mask};
    use isotrail_testkit::{bundle, grid};

    use super::*;
    use crate::{Mesher, SmoothMesher, VoxelMesher};

    fn forest(cols: u32, rows: u32, class: Landcover) -> SceneBundle {
        let grid = grid(cols, rows, 10.0);
        let heights: Vec<f32> = (0..grid.cells()).map(|i| (i % 7) as f32 * 3.0).collect();
        bundle(grid, &heights, &vec![class; grid.cells()])
    }

    fn props(bundle: &SceneBundle, mesher: &dyn Mesher, metres_per_px: f32) -> TerrainMesh {
        let terrain = mesher.mesh(bundle);
        let mut mesh = terrain.clone();
        mesh.add_props(bundle, metres_per_px);
        mesh.vertices.drain(..terrain.vertices.len());
        mesh
    }

    /// Each box's colour, footprint centre and bottom; a box is a top quad
    /// then four walls, six vertices each
    fn boxes(mesh: &TerrainMesh) -> Vec<([u8; 4], Vec2, f32)> {
        mesh.vertices
            .chunks(30)
            .map(|b| {
                let top = b[..6].iter().map(|v| Vec3::from(v.position).truncate());
                let bottom = b
                    .iter()
                    .map(|v| v.position[2])
                    .fold(f32::INFINITY, f32::min);
                (b[0].colour, top.sum::<Vec2>() / 6.0, bottom)
            })
            .collect()
    }

    /// Where each prop stands, in slab metres
    fn footprints(mesh: &TerrainMesh) -> Vec<Vec2> {
        let mut centres: Vec<Vec2> = boxes(mesh).into_iter().map(|b| b.1).collect();
        centres.dedup_by(|a, b| a.distance(*b) < 1e-3);
        centres
    }

    #[test]
    fn props_are_seeded_by_the_bundle() {
        let mut bundle = forest(40, 20, Landcover::MixedForest);
        let first = props(&bundle, &VoxelMesher::default(), 5.0);
        assert!(!first.vertices.is_empty());
        assert_eq!(first, props(&bundle, &VoxelMesher::default(), 5.0));
        bundle.prop_seed ^= 1;
        assert_ne!(first, props(&bundle, &VoxelMesher::default(), 5.0));
    }

    #[test]
    fn only_forest_scrub_and_scree_carry_props() {
        for class in Landcover::ALL {
            let mut bundle = forest(40, 20, class);
            let mut mesh = VoxelMesher::default().mesh(&bundle);
            let count = mesh.add_props(&bundle, 5.0);
            let expected =
                class.is_forest() || matches!(class, Landcover::Scrub | Landcover::Scree);
            assert_eq!(count > 0, expected, "{class:?}");
            if expected {
                bundle.terrain.lines.fill(line_mask::PATH);
                let mut mesh = VoxelMesher::default().mesh(&bundle);
                assert_eq!(mesh.add_props(&bundle, 5.0), 0, "{class:?} on paths");
            }
        }
    }

    #[test]
    fn colours_tell_conifers_from_broadleaves() {
        let colours = |class| {
            let mesh = props(&forest(40, 20, class), &VoxelMesher::default(), 5.0);
            let mut colours: Vec<[u8; 4]> = mesh.vertices.iter().map(|v| v.colour).collect();
            colours.sort_unstable();
            colours.dedup();
            colours
        };
        assert_eq!(colours(Landcover::Coniferous), [NEEDLES, TRUNK]);
        assert_eq!(colours(Landcover::Broadleaf), [LEAVES, TRUNK]);
        assert_eq!(colours(Landcover::Scrub), [BUSH]);
        assert_eq!(colours(Landcover::Scree), [BOULDER]);
        assert_eq!(colours(Landcover::MixedForest).len(), 3);
    }

    #[test]
    fn props_stand_on_the_ground_without_floating() {
        let bundle = forest(40, 20, Landcover::Coniferous);
        for mesher in [&VoxelMesher::default() as &dyn Mesher, &SmoothMesher] {
            let terrain = mesher.mesh(&bundle);
            let mesh = props(&bundle, mesher, 5.0);
            let to_grid = |p: Vec2| Vec2::new(p.x / 10.0 + 20.0, 10.0 - p.y / 10.0);
            let trunks: Vec<_> = boxes(&mesh).into_iter().filter(|b| b.0 == TRUNK).collect();
            assert!(trunks.len() > 50, "{}", trunks.len());
            for (_, centre, bottom) in trunks {
                let at = to_grid(centre);
                let under = terrain.surface.height(at, at);
                // Sunk no deeper than the steepest step under a crown
                assert!(bottom <= under + 1e-3, "{bottom} over {under}");
                assert!(bottom >= under - 20.0 - 1e-3, "{bottom} under {under}");
            }
        }
    }

    #[test]
    fn props_are_measured_in_pixels() {
        let bundle = forest(80, 40, Landcover::Broadleaf);
        let crown_width = |metres_per_px: f32| {
            let mesh = props(&bundle, &VoxelMesher::default(), metres_per_px);
            let top = mesh
                .triangles()
                .find(|t| t[0].colour == LEAVES && t[0].normal == [0.0, 0.0, 1.0])
                .unwrap();
            let xs = top.iter().map(|v| v.position[0]);
            xs.clone().fold(f32::NEG_INFINITY, f32::max) - xs.fold(f32::INFINITY, f32::min)
        };
        assert!((crown_width(4.0) - 12.0).abs() < 1e-3);
        assert!((crown_width(8.0) - 24.0).abs() < 1e-3);
        let count = |metres_per_px| {
            let mut mesh = VoxelMesher::default().mesh(&bundle);
            mesh.add_props(&bundle, metres_per_px)
        };
        assert!(
            count(4.0) > 3 * count(8.0),
            "{} vs {}",
            count(4.0),
            count(8.0)
        );
    }

    #[test]
    fn huge_slabs_stay_bounded() {
        let bundle = forest(300, 150, Landcover::Coniferous);
        let mut mesh = VoxelMesher::default().mesh(&bundle);
        let count = mesh.add_props(&bundle, 0.1);
        assert!(count > MAX_SPOTS / 2 && count <= MAX_SPOTS, "{count}");
    }

    #[test]
    fn the_route_runs_through_a_clearing() {
        let mut bundle = forest(80, 40, Landcover::Coniferous);
        let point = |col: f32| RoutePoint {
            col,
            row: 20.0,
            distance_m: col * 10.0,
            time: None,
            gps_ele_m: None,
            draped_m: 0.0,
        };
        bundle.route.points = vec![point(0.0), point(30.0), point(50.0), point(80.0)];
        bundle.route.breaks = vec![2];
        let metres_per_px = 5.0;
        let mesh = props(&bundle, &VoxelMesher::default(), metres_per_px);
        let to_grid = |p: Vec2| Vec2::new(p.x / 10.0 + 40.0, 20.0 - p.y / 10.0);
        let clearing = (CLEARING_PX + 1.5) * metres_per_px / 10.0;
        let centres = footprints(&mesh);
        let (mut beside, mut in_gap) = (0, 0);
        for centre in centres.iter().map(|&c| to_grid(c)) {
            let off = (centre.y - 20.0).abs();
            if (30.0..50.0).contains(&centre.x) {
                in_gap += usize::from(off < clearing);
            } else {
                assert!(off >= clearing - 1e-3, "{centre}");
                beside += usize::from(off < clearing * 3.0);
            }
        }
        assert!(beside > 5, "{beside}");
        assert!(in_gap > 0, "the gap between pieces isn't cleared");
    }
}
