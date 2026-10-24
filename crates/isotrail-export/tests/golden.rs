use isotrail_export::{render_still, upscale};
use isotrail_render::{
    Aspect, BACKGROUND, DEFAULT_YAW_OFFSET_DEG, Gpu, Mesher, OffscreenTarget, SmoothMesher,
    VoxelMesher, draw_test_scene,
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

fn assert_terrain_golden(name: &str, mesher: &dyn Mesher) {
    let gpu = Gpu::headless().unwrap();
    let framing = Aspect::Story.framing(6).unwrap();
    let frame = render_still(
        &gpu,
        &diorama_bundle(),
        mesher,
        &framing,
        DEFAULT_YAW_OFFSET_DEG,
    )
    .unwrap();
    assert_eq!((frame.width(), frame.height()), (180, 320));
    // The safe area's corners stay empty and its centre is terrain
    assert_eq!(frame.pixel(0, 0), BACKGROUND);
    assert_eq!(frame.pixel(179, 319), BACKGROUND);
    assert_ne!(frame.pixel(90, 166), BACKGROUND);
    assert_golden(name, frame.width(), frame.height(), frame.rgba());
}

#[test]
fn smooth_terrain_matches_golden() {
    assert_terrain_golden("terrain-smooth", &SmoothMesher);
}

#[test]
fn voxel_terrain_matches_golden() {
    assert_terrain_golden("terrain-voxel", &VoxelMesher::default());
}
