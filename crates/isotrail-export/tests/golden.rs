use isotrail_export::{render_still, upscale};
use isotrail_render::{
    Aspect, DEFAULT_PALETTE, DEFAULT_YAW_OFFSET_DEG, Gpu, Look, Mesher, OffscreenTarget, Palette,
    SmoothMesher, VoxelMesher, draw_test_scene,
};
use isotrail_testkit::{assert_golden, diorama_bundle};

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
