use std::collections::HashSet;
use std::f32::consts::FRAC_PI_2;

use glam::{Vec2, Vec3};
use isotrail_bundle::{Landcover, SceneBundle};
use isotrail_render::{
    Aspect, BANDS, Camera, Frame, Gpu, Look, Mesher, OffscreenTarget, Scene, SmoothMesher,
    SunState, TerrainRenderer, VoxelMesher,
};
use isotrail_testkit::{bundle, grid};

const SIZE: u32 = 81;
const CELL_M: f32 = 10.0;

fn meshers() -> [(&'static str, Box<dyn Mesher>); 2] {
    [
        ("smooth", Box::new(SmoothMesher)),
        ("voxel", Box::new(VoxelMesher::default())),
    ]
}

/// A flat grass slab with heights from `height`, given the cell's offset
/// from the centre cell
fn slab(height: impl Fn(i32, i32) -> f32) -> SceneBundle {
    let centre = (SIZE / 2) as i32;
    let heights: Vec<f32> = (0..SIZE * SIZE)
        .map(|i| height((i % SIZE) as i32 - centre, (i / SIZE) as i32 - centre))
        .collect();
    let classes = vec![Landcover::Grassland; heights.len()];
    bundle(grid(SIZE, SIZE, CELL_M.into()), &heights, &classes)
}

struct Render {
    frame: Frame,
    camera: Camera,
    sun: SunState,
}

impl Render {
    /// Looks along slab-frame angle `heading` under the late-morning sun, with
    /// no outlines or palette
    fn new(bundle: &SceneBundle, mesher: &dyn Mesher, heading: f32) -> Render {
        let gpu = Gpu::headless().unwrap();
        let framing = Aspect::Story.framing(3).unwrap();
        let mesh = mesher.mesh(bundle);
        let camera = Camera::diorama(heading, mesh.positions(), &framing);
        let sun = SunState::late_morning(&bundle.grid);
        let renderer = TerrainRenderer::new(&gpu);
        let target = OffscreenTarget::new(&gpu, framing.width, framing.height);
        let mesh = renderer.upload(&gpu, &mesh);
        renderer.draw(
            &gpu,
            &target,
            &Scene::terrain(&mesh),
            &camera,
            &sun,
            &Look::plain(),
        );
        Render {
            frame: target.read_back(&gpu).unwrap(),
            camera,
            sun,
        }
    }

    /// The colour at a slab-frame point on the terrain's top
    fn at(&self, point: Vec3) -> [u8; 4] {
        let ndc = self.camera.view_proj.project_point3(point);
        let x = (ndc.x + 1.0) / 2.0 * self.frame.width() as f32;
        let y = (1.0 - ndc.y) / 2.0 * self.frame.height() as f32;
        self.frame.pixel(x as u32, y as u32)
    }

    /// The left column is always outside the safe area, so it holds every
    /// row's sky colour
    fn sky(&self) -> HashSet<[u8; 4]> {
        (0..self.frame.height())
            .map(|y| self.frame.pixel(0, y))
            .collect()
    }
}

/// Points on a flat top at height `z`, keeping `margin_m` from the slab edges
fn top_points(z: f32, margin_m: f32) -> impl Iterator<Item = Vec3> {
    let half = SIZE as f32 * CELL_M / 2.0 - margin_m;
    (0..=20).flat_map(move |i| {
        (0..=20).map(move |j| {
            Vec3::new(
                -half + i as f32 * half / 10.0,
                -half + j as f32 * half / 10.0,
                z,
            )
        })
    })
}

#[test]
fn flat_ground_is_one_colour_without_acne() {
    let flat = slab(|_, _| 0.0);
    for (name, mesher) in meshers() {
        for heading in [-FRAC_PI_2, 0.6, 2.5] {
            let render = Render::new(&flat, mesher.as_ref(), heading);
            let colours: HashSet<[u8; 4]> = top_points(0.0, CELL_M).map(|p| render.at(p)).collect();
            assert_eq!(colours.len(), 1, "{name} at {heading}: {colours:?}");
            assert!(colours.is_disjoint(&render.sky()), "{name}");
        }
    }
}

#[test]
fn a_tower_casts_its_shadow_away_from_the_sun() {
    // A 30 Ã— 30 m tower, 200 m tall
    let tower = slab(|c, r| {
        if c.abs() <= 1 && r.abs() <= 1 {
            200.0
        } else {
            0.0
        }
    });
    for (name, mesher) in meshers() {
        // Looking towards the sun, so the shadow lies in front of the tower
        let sun = SunState::late_morning(&tower.grid).direction;
        let towards_sun = Vec2::new(sun.x, sun.y).normalize();
        let render = Render::new(&tower, mesher.as_ref(), towards_sun.to_angle());
        assert_eq!(render.sun.direction, sun);

        let ground = |along: f32, across: f32| {
            let offset = towards_sun * along + towards_sun.perp() * across;
            render.at(offset.extend(0.0))
        };
        let open = ground(0.0, 120.0);
        assert_eq!(ground(0.0, -120.0), open, "{name}");
        // The shadow is about 240 m long
        let shade = ground(-80.0, 0.0);
        assert_ne!(shade, open, "{name}");
        assert_eq!(ground(-160.0, 0.0), shade, "{name}");
        assert_eq!(ground(-300.0, 0.0), open, "{name}");
        let brightness = |[r, g, b, _]: [u8; 4]| u32::from(r) + u32::from(g) + u32::from(b);
        assert!(brightness(shade) < brightness(open), "{name}");
    }
}

#[test]
fn light_falls_in_a_few_bands() {
    let hill = slab(|c, r| 300.0 * (-((c * c + r * r) as f32) / 60.0).exp());
    let render = Render::new(&hill, &SmoothMesher, 0.3);
    let sky = render.sky();
    let colours: HashSet<[u8; 4]> = top_points(0.0, 4.0 * CELL_M)
        .map(|p| {
            let (c, r) = (p.x / CELL_M, p.y / CELL_M);
            let z = 300.0 * (-(c * c + r * r) / 60.0).exp();
            render.at(p.with_z(z))
        })
        .filter(|c| !sky.contains(c))
        .collect();
    let levels = (BANDS + 1) as usize;
    assert!(colours.len() > 2, "{colours:?}");
    assert!(colours.len() <= levels * levels, "{}", colours.len());
}
