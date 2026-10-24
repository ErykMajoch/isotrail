use std::collections::HashSet;

use isotrail_bundle::{Landcover, SceneBundle};
use isotrail_render::{
    Aspect, Camera, DEFAULT_PALETTE, Edges, Frame, Gpu, Look, Mesher, OffscreenTarget, Palette,
    SmoothMesher, SunState, TerrainRenderer, VoxelMesher,
};
use isotrail_testkit::{bundle, diorama_bundle, grid};

const HEADING: f32 = 0.6;

fn render(bundle: &SceneBundle, mesher: &dyn Mesher, look: &Look) -> Frame {
    let gpu = Gpu::headless().unwrap();
    let framing = Aspect::Story.framing(3).unwrap();
    let mesh = mesher.mesh(bundle);
    let camera = Camera::diorama(HEADING, mesh.positions(), &framing);
    let renderer = TerrainRenderer::new(&gpu);
    let target = OffscreenTarget::new(&gpu, framing.width, framing.height);
    let mesh = renderer.upload(&gpu, &mesh);
    let sun = SunState::late_morning(&bundle.grid);
    renderer.draw(&gpu, &target, &mesh, &camera, &sun, look);
    target.read_back(&gpu).unwrap()
}

fn flat_slab() -> SceneBundle {
    let size = 61;
    let cells = (size * size) as usize;
    bundle(
        grid(size, size, 10.0),
        &vec![0.0; cells],
        &vec![Landcover::Grassland; cells],
    )
}

fn rgb([r, g, b, _]: [u8; 4]) -> [u8; 3] {
    [r, g, b]
}

fn brightness(pixel: [u8; 4]) -> u32 {
    rgb(pixel).iter().map(|&c| u32::from(c)).sum()
}

fn pixels(frame: &Frame) -> impl Iterator<Item = (u32, u32)> + '_ {
    (0..frame.height()).flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
}

/// Terrain pixels of an undithered frame: those that differ from their
/// row's sky colour in the left column, which never holds terrain
fn terrain(frame: &Frame) -> HashSet<(u32, u32)> {
    pixels(frame)
        .filter(|&(x, y)| frame.pixel(x, y) != frame.pixel(0, y))
        .collect()
}

fn neighbours((x, y): (u32, u32)) -> [(u32, u32); 4] {
    [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
}

#[test]
fn quantised_frames_use_only_palette_colours() {
    for name in Palette::builtin_names() {
        let palette = Palette::builtin(name).unwrap();
        let allowed: HashSet<[u8; 3]> = palette.colours().iter().copied().collect();
        let look = Look {
            palette: Some(palette),
            ..Look::default()
        };
        for mesher in [&SmoothMesher as &dyn Mesher, &VoxelMesher::default()] {
            let frame = render(&diorama_bundle(), mesher, &look);
            let used: HashSet<[u8; 3]> = pixels(&frame)
                .map(|(x, y)| rgb(frame.pixel(x, y)))
                .collect();
            assert!(used.is_subset(&allowed), "{name}");
            assert!(used.len() > 4, "{name}: {}", used.len());
        }
    }
}

#[test]
fn the_gpu_picks_the_same_colours_as_the_cpu() {
    let palette = Palette::builtin(DEFAULT_PALETTE).unwrap();
    let diorama = diorama_bundle();
    let plain = render(&diorama, &SmoothMesher, &Look::plain());
    let quantised = render(
        &diorama,
        &SmoothMesher,
        &Look {
            palette: Some(palette.clone()),
            ..Look::plain()
        },
    );
    let total = plain.width() * plain.height();
    let mismatches = pixels(&plain)
        .filter(|&(x, y)| rgb(quantised.pixel(x, y)) != palette.nearest(rgb(plain.pixel(x, y))))
        .count();
    // Near-ties may round either way
    assert!(mismatches as f64 <= total as f64 * 0.002, "{mismatches}");
}

#[test]
fn only_the_sky_is_dithered() {
    let flat = flat_slab();
    let frame = render(&flat, &VoxelMesher::default(), &Look::default());
    let undithered = render(
        &flat,
        &VoxelMesher::default(),
        &Look {
            sky_dither: 0.0,
            ..Look::default()
        },
    );
    for (x, y) in terrain(&undithered) {
        assert_eq!(frame.pixel(x, y), undithered.pixel(x, y), "({x}, {y})");
    }

    let row_varies = |frame: &Frame, y: u32| (1..8).any(|x| frame.pixel(x, y) != frame.pixel(0, y));
    let dithered_rows = (0..frame.height())
        .filter(|&y| row_varies(&frame, y))
        .count();
    assert!(dithered_rows > 10, "{dithered_rows}");
    assert!((0..frame.height()).all(|y| !row_varies(&undithered, y)));
}

#[test]
fn silhouettes_darken_and_the_rim_lightens() {
    let flat = flat_slab();
    let mesher = VoxelMesher::default();
    let plain = render(&flat, &mesher, &Look::plain());
    let outlined = render(
        &flat,
        &mesher,
        &Look {
            edges: Edges::default(),
            ..Look::plain()
        },
    );
    let ground = terrain(&plain);
    let silhouette: Vec<(u32, u32)> = ground
        .iter()
        .copied()
        .filter(|&p| neighbours(p).iter().any(|n| !ground.contains(n)))
        .collect();
    assert!(silhouette.len() > 100, "{}", silhouette.len());
    for &(x, y) in &silhouette {
        assert!(
            brightness(outlined.pixel(x, y)) < brightness(plain.pixel(x, y)),
            "({x}, {y})"
        );
    }

    // The top's front rim: top-coloured pixels with a cut face below them
    let (cx, cy) = (plain.width() / 2, plain.height() / 2);
    let top = plain.pixel(cx, cy);
    let rim: Vec<(u32, u32)> = ground
        .iter()
        .copied()
        .filter(|&(x, y)| {
            plain.pixel(x, y) == top
                && neighbours((x, y)).iter().all(|n| ground.contains(n))
                && plain.pixel(x, y + 1) != top
        })
        .collect();
    assert!(rim.len() > 50, "{}", rim.len());
    for &(x, y) in &rim {
        assert!(
            brightness(outlined.pixel(x, y)) > brightness(top),
            "({x}, {y})"
        );
    }
    // Away from every edge, nothing changes
    assert_eq!(outlined.pixel(cx, cy), top);
}
