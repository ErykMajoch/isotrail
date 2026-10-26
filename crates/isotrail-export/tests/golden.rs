use isotrail_bundle::Units;
use isotrail_export::{ExportError, GifWriter, render_clip, render_still, upscale};
use isotrail_render::{
    Aspect, Background, DEFAULT_PALETTE, DEFAULT_STATS, DEFAULT_YAW_OFFSET_DEG, DateFormat, Frame,
    Gpu, Look, Mesher, Motion, OffscreenTarget, OverlayStyle, OverlayText, Overlays, Pace, Palette,
    SmoothMesher, Timeline, VoxelMesher, Walk, draw_test_scene,
};
use isotrail_testkit::{assert_golden, decode_gif, diorama_bundle, hike_bundle};

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

/// A one-second, seven-frame square clip of the synthetic hike, paced by its
/// times, with the stats counting up, decoded back from the GIF
#[test]
fn animation_frames_match_goldens() {
    let gpu = Gpu::headless().unwrap();
    let mut bundle = hike_bundle();
    // Ten minutes between points, with an hour's stop at the fourth
    for (i, point) in bundle.route.points.iter_mut().enumerate() {
        let stop = if i >= 4 { 3_600 } else { 0 };
        point.time = Some(1_750_000_000 + i as i64 * 600 + stop);
    }
    let framing = Aspect::Square.framing(6).unwrap();
    let units = Units::Metric;
    let text = OverlayText::from_bundle(&bundle, units, &DEFAULT_STATS, DateFormat::Long);
    let overlays = Overlays::layout(&text, &OverlayStyle::default(), &framing);
    let look = Look::default();
    let motion = Motion {
        timeline: Timeline {
            duration_s: 1.0,
            fps: 7,
            step_fps: None,
        },
        pace: Pace::Time,
        ..Motion::default()
    };
    let walk = Walk::new(&bundle);
    assert!(walk.is_timed());
    let diorama = overlays.diorama_framing();
    let mut gif = GifWriter::new(Vec::new(), diorama.width, diorama.height, 1, &look, 7).unwrap();
    render_clip(
        &gpu,
        &bundle,
        &VoxelMesher::default(),
        &diorama,
        &look,
        &motion,
        |moment, frame| -> Result<(), ExportError> {
            let walked = walk.stats(moment.progress);
            let stats: Vec<_> = DEFAULT_STATS
                .iter()
                .filter_map(|kind| kind.stat_walked(&bundle.meta.stats, &walked, units))
                .collect();
            let mut frame = frame.clone();
            overlays
                .with_stats(&stats)
                .draw(&mut frame, look.palette.as_ref(), moment.progress);
            gif.push(&frame)
        },
    )
    .unwrap();
    let frames = decode_gif(&gif.finish().unwrap());
    assert_eq!(frames.len(), 7);
    assert_eq!(
        frames.iter().map(|f| u32::from(f.delay_cs)).sum::<u32>(),
        100
    );
    // The last frame is in the hold
    for (name, i) in [("first", 0), ("middle", 3), ("last", 6)] {
        let frame = &frames[i];
        assert_eq!((frame.width, frame.height), (180, 180));
        assert_golden(
            &format!("animation-{name}"),
            frame.width,
            frame.height,
            &frame.rgba,
        );
    }
}
