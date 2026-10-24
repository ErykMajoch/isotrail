use isotrail_export::upscale;
use isotrail_render::{Gpu, OffscreenTarget, draw_test_scene};
use isotrail_testkit::assert_golden;

#[test]
fn test_scene_matches_golden() {
    let gpu = Gpu::headless().unwrap();
    let target = OffscreenTarget::new(&gpu, 90, 60);
    draw_test_scene(&gpu, &target);
    let frame = upscale(&target.read_back(&gpu).unwrap(), 3).unwrap();
    assert_golden("test-scene", frame.width(), frame.height(), frame.rgba());
}
