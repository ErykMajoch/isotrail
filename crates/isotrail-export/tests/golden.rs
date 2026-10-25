use isotrail_bundle::Units;
use isotrail_export::{render_still, upscale};
use isotrail_render::{
    Aspect, Background, DEFAULT_PALETTE, DEFAULT_STATS, DEFAULT_YAW_OFFSET_DEG, DateFormat, Frame,
    Gpu, Look, Mesher, OffscreenTarget, OverlayStyle, OverlayText, Overlays, Palette, SmoothMesher,
    VoxelMesher, draw_test_scene,
};
use isotrail_testkit::{assert_golden, diorama_bundle, hike_bundle};

#[test]
fn test_scene_matches_golden() {
    let gpu = Gpu::headless().unwrap();
    let target = OffscreenTarget::new(&gpu, 90, 60);
    draw_test_scene(&gpu, &target);
    let frame = upscale(&target.read_back(&gpu).unwrap(), 3).unwrap();
    assert_golden("test-scene", frame.width(), frame.height(), frame.rgba());
}

fn assert_terrain_golden(name: &str, mesher: &dyn Mesher, look: &Look) {
    let gpu = Gpu::headless().unwrap();
    let framing = Aspect::Story.framing(6).unwrap();
    let frame = render_still(
        &gpu,
        &diorama_bundle(),
        mesher,
        &framing,
        DEFAULT_YAW_OFFSET_DEG,
        look,
    )
    .unwrap();
    assert_eq!((frame.width(), frame.height()), (180, 320));
    // The safe area's corners stay sky and its centre is terrain. The sky's
    // dither repeats every 4 pixels along a row, and the left edge is all sky
    let sky = |x: u32, y| frame.pixel(x, y) == frame.pixel(x % 4, y);
    assert!(sky(179, 0) && sky(179, 319));
    assert!(!sky(90, 166));
    assert_golden(name, frame.width(), frame.height(), frame.rgba());
}

#[test]
fn smooth_terrain_matches_golden() {
    assert_terrain_golden("terrain-smooth", &SmoothMesher, &Look::default());
}

#[test]
fn voxel_terrain_matches_golden() {
    assert_terrain_golden("terrain-voxel", &VoxelMesher::default(), &Look::default());
}

#[test]
fn other_palettes_match_goldens() {
    for name in Palette::builtin_names().filter(|&name| name != DEFAULT_PALETTE) {
        let look = Look {
            palette: Palette::builtin(name),
            ..Look::default()
        };
        assert_terrain_golden(&format!("palette-{name}"), &VoxelMesher::default(), &look);
    }
}

#[test]
fn overlays_on_the_diorama_match_golden() {
    let gpu = Gpu::headless().unwrap();
    let mut bundle = diorama_bundle();
    bundle.meta.title = "Zażółć gęślą jaźń | Łańcuch".into();
    bundle.meta.local_date = Some("2025-06-14".into());
    let stats = &mut bundle.meta.stats;
    (stats.distance_m, stats.gain_m) = (7_420.0, Some(612.0));
    (stats.moving_s, stats.max_ele_m) = (Some(9_000), Some(842.0));
    let framing = Aspect::Story.framing(6).unwrap();
    let text = OverlayText::from_bundle(&bundle, Units::Metric, &DEFAULT_STATS, DateFormat::Long);
    assert_eq!(text.date.as_deref(), Some("Saturday 14 June 2025"));
    let overlays = Overlays::layout(&text, &OverlayStyle::default(), &framing);
    let look = Look::default();
    let mut frame = render_still(
        &gpu,
        &bundle,
        &VoxelMesher::default(),
        &overlays.diorama_framing(),
        DEFAULT_YAW_OFFSET_DEG,
        &look,
    )
    .unwrap();
    overlays.draw(&mut frame, look.palette.as_ref(), 1.0);
    assert_golden(
        "overlay-diorama",
        frame.width(),
        frame.height(),
        frame.rgba(),
    );
}

fn render_hike(gpu: &Gpu, aspect: Aspect, look: &Look) -> Frame {
    let bundle = hike_bundle();
    let framing = aspect.framing(6).unwrap();
    let text = OverlayText::from_bundle(&bundle, Units::Metric, &DEFAULT_STATS, DateFormat::Long);
    let overlays = Overlays::layout(&text, &OverlayStyle::default(), &framing);
    let mut frame = render_still(
        gpu,
        &bundle,
        &VoxelMesher::default(),
        &overlays.diorama_framing(),
        DEFAULT_YAW_OFFSET_DEG,
        look,
    )
    .unwrap();
    overlays.draw(&mut frame, look.palette.as_ref(), 1.0);
    frame
}

#[test]
fn every_preset_matches_its_golden() {
    let gpu = Gpu::headless().unwrap();
    for aspect in Aspect::ALL {
        let frame = render_hike(&gpu, aspect, &Look::default());
        let (width, height) = aspect.output_size();
        assert_eq!((frame.width(), frame.height()), (width / 6, height / 6));
        assert_golden(
            &format!("preset-{}", aspect.name()),
            frame.width(),
            frame.height(),
            frame.rgba(),
        );
    }
}

#[test]
fn backgrounds_replace_only_the_sky() {
    let gpu = Gpu::headless().unwrap();
    let sky = render_hike(&gpu, Aspect::Square, &Look::default());
    let render = |background| {
        let look = Look {
            background,
            ..Look::default()
        };
        render_hike(&gpu, Aspect::Square, &look)
    };
    let transparent = render(Background::Transparent);
    let colour = [12, 200, 99];
    let flat = render(Background::Colour(colour));
    // The corners are sky, and the diorama sits above the stats panel
    for (x, y) in [(0, 0), (179, 0), (0, 179), (179, 179)] {
        assert_eq!(transparent.pixel(x, y), [0, 0, 0, 0], "({x}, {y})");
        assert_eq!(flat.pixel(x, y), [12, 200, 99, 255], "({x}, {y})");
    }
    assert_ne!(sky.pixel(90, 52), sky.pixel(90 % 4, 52));
    assert_eq!(transparent.pixel(90, 52), sky.pixel(90, 52));
    assert_eq!(flat.pixel(90, 52), sky.pixel(90, 52));
}
