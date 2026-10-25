use glam::{Vec2, Vec3};
use isotrail_bundle::{Landcover, Route, RoutePoint, SceneBundle};
use isotrail_render::{
    Aspect, Camera, Frame, Gpu, Look, Mesher, OffscreenTarget, RouteLine, RouteStyle, Scene,
    SmoothMesher, SunState, TerrainRenderer, VoxelMesher,
};
use isotrail_testkit::{assert_golden, bundle, grid};

const COLS: u32 = 80;
const ROWS: u32 = 40;
const CELL_M: f32 = 10.0;
const ROUTE_ROW: f32 = 20.5;
/// Looking roughly along the route, so the ridge hides its far slope
const HEADING: f32 = 0.15;
const HEAD_CORE: [u8; 3] = [235, 237, 233];
const HEAD_RING: [u8; 3] = [232, 193, 112];

/// A grass slab with a 150 m ridge across its middle, at column 40
fn ridge(route: &[(f32, f32)], breaks: Vec<u32>) -> SceneBundle {
    let heights: Vec<f32> = (0..COLS * ROWS)
        .map(|i| {
            let c = (i % COLS) as f32 + 0.5;
            150.0 * (-((c - 40.0) / 6.0).powi(2)).exp()
        })
        .collect();
    let classes = vec![Landcover::Grassland; heights.len()];
    let mut bundle = bundle(grid(COLS, ROWS, CELL_M.into()), &heights, &classes);
    let mut distance_m = 0.0;
    let mut previous = route[0];
    bundle.route = Route {
        points: route
            .iter()
            .map(|&(col, row)| {
                distance_m += (col - previous.0).hypot(row - previous.1) * CELL_M;
                previous = (col, row);
                RoutePoint {
                    col,
                    row,
                    distance_m,
                    time: None,
                    gps_ele_m: None,
                    draped_m: 0.0,
                }
            })
            .collect(),
        breaks,
    };
    bundle
}

/// Straight over the ridge, from column 5 to 75
fn over_the_ridge() -> SceneBundle {
    let points: Vec<(f32, f32)> = (0..=70).map(|i| (5.0 + i as f32, ROUTE_ROW)).collect();
    ridge(&points, Vec::new())
}

struct Render {
    frame: Frame,
    camera: Camera,
    line: RouteLine,
}

impl Render {
    fn new(
        bundle: &SceneBundle,
        mesher: &dyn Mesher,
        look: &Look,
        progress: Option<f32>,
    ) -> Render {
        let gpu = Gpu::headless().unwrap();
        let framing = Aspect::Story.framing(3).unwrap();
        let mesh = mesher.mesh(bundle);
        let camera = Camera::diorama(HEADING, mesh.positions(), &framing);
        let line = RouteLine::drape(bundle, &mesh.surface, camera.metres_per_px / 2.0);
        let renderer = TerrainRenderer::new(&gpu);
        let target = OffscreenTarget::new(&gpu, framing.width, framing.height);
        let gpu_mesh = renderer.upload(&gpu, &mesh);
        let scene = Scene {
            mesh: &gpu_mesh,
            route: progress.is_some().then_some(&line),
            progress: progress.unwrap_or(1.0),
        };
        let sun = SunState::late_morning(&bundle.grid);
        renderer.draw(&gpu, &target, &scene, &camera, &sun, look);
        Render {
            frame: target.read_back(&gpu).unwrap(),
            camera,
            line,
        }
    }

    fn pixel_of(&self, point: Vec3) -> (u32, u32) {
        let ndc = self.camera.view_proj.project_point3(point);
        let at = Vec2::new(
            (ndc.x + 1.0) / 2.0 * self.frame.width() as f32,
            (1.0 - ndc.y) / 2.0 * self.frame.height() as f32,
        );
        (at.x as u32, at.y as u32)
    }

    /// The route's point at `col`, on the ground
    fn route_at(&self, col: f32) -> Vec3 {
        self.line.point_at((col - 5.0) * CELL_M).unwrap()
    }

    /// Colours within a pixel of `point`
    fn around(&self, point: Vec3) -> Vec<[u8; 3]> {
        let (x, y) = self.pixel_of(point);
        let mut colours = Vec::new();
        for dy in 0..3 {
            for dx in 0..3 {
                let [r, g, b, _] = self.frame.pixel(x + dx - 1, y + dy - 1);
                colours.push([r, g, b]);
            }
        }
        colours
    }

    fn shows_route(&self, col: f32) -> bool {
        self.around(self.route_at(col))
            .contains(&RouteStyle::default().colour)
    }
}

#[test]
fn the_ridge_hides_the_far_slope() {
    let bundle = over_the_ridge();
    let mesher = VoxelMesher::default();
    let drawn = Render::new(&bundle, &mesher, &Look::plain(), Some(1.0));
    let bare = Render::new(&bundle, &mesher, &Look::plain(), None);

    for col in [10.0, 20.0, 30.0, 70.0] {
        assert!(drawn.shows_route(col), "column {col}");
    }
    let mut hidden_marks = 0;
    for col in (46..=60).map(|c| c as f32) {
        assert!(!drawn.shows_route(col), "column {col}");
        let point = drawn.route_at(col);
        if drawn.around(point) != bare.around(point) {
            hidden_marks += 1;
        }
    }
    // Dashed, so not every point is marked
    assert!(hidden_marks >= 5, "{hidden_marks}");
}

#[test]
fn progress_draws_part_of_the_route_up_to_the_head() {
    let bundle = over_the_ridge();
    let mesher = SmoothMesher;
    let half = Render::new(&bundle, &mesher, &Look::plain(), Some(0.5));
    assert!(half.shows_route(20.0));
    assert!(!half.shows_route(70.0));
    let head = half.line.point_at(half.line.distance_at(0.5)).unwrap();
    let (x, y) = half.pixel_of(head);
    let [r, g, b, _] = half.frame.pixel(x, y);
    assert_eq!([r, g, b], HEAD_CORE);
    assert!(half.frame.rgba().chunks(4).any(|p| p[..3] == HEAD_RING));

    let none = Render::new(&bundle, &mesher, &Look::plain(), Some(0.0));
    assert!(!none.shows_route(20.0));
    let full = Render::new(&bundle, &mesher, &Look::plain(), Some(1.0));
    assert!(full.shows_route(70.0));
    assert!(!full.frame.rgba().chunks(4).any(|p| p[..3] == HEAD_RING));
}

#[test]
fn gaps_stay_open() {
    let mut points: Vec<(f32, f32)> = (0..=20).map(|i| (5.0 + i as f32, ROUTE_ROW)).collect();
    points.extend((0..=10).map(|i| (65.0 + i as f32, ROUTE_ROW)));
    let bundle = ridge(&points, vec![21]);
    let render = Render::new(&bundle, &VoxelMesher::default(), &Look::plain(), Some(1.0));
    assert_eq!(render.line.pieces().len(), 2);
    assert!(render.shows_route(15.0));
    // Where a line from the first piece's end to the second's start would run
    let across = render.line.pieces()[0]
        .last()
        .unwrap()
        .position
        .lerp(render.line.pieces()[1][0].position, 0.2);
    assert!(
        !render
            .around(across)
            .contains(&RouteStyle::default().colour)
    );
}

#[test]
fn route_over_a_ridge_matches_goldens() {
    let bundle = over_the_ridge();
    for (name, mesher) in [
        ("route-ridge-voxel", &VoxelMesher::default() as &dyn Mesher),
        ("route-ridge-smooth", &SmoothMesher),
    ] {
        let render = Render::new(&bundle, mesher, &Look::default(), Some(1.0));
        let frame = &render.frame;
        assert_golden(name, frame.width(), frame.height(), frame.rgba());
    }
}
