use isotrail_render::{Frame, Gpu, OffscreenTarget, Present, TARGET_FORMAT, draw_test_scene};

fn present(gpu: &Gpu, frame: &OffscreenTarget, (width, height): (u32, u32), scale: u32) -> Frame {
    let screen = OffscreenTarget::new(gpu, width, height);
    Present::new(gpu, TARGET_FORMAT).draw(gpu, frame, screen.view(), (width, height), scale);
    screen.read_back(gpu).unwrap()
}

#[test]
fn each_pixel_becomes_a_square_block() {
    let gpu = Gpu::headless().unwrap();
    let frame = OffscreenTarget::new(&gpu, 90, 60);
    draw_test_scene(&gpu, &frame);
    let source = frame.read_back(&gpu).unwrap();

    let screen = present(&gpu, &frame, (270, 180), 3);
    for y in 0..180 {
        for x in 0..270 {
            assert_eq!(
                screen.pixel(x, y),
                source.pixel(x / 3, y / 3),
                "pixel ({x}, {y})"
            );
        }
    }
}

#[test]
fn frame_is_centred_and_cropped_or_bordered() {
    let gpu = Gpu::headless().unwrap();
    let frame = OffscreenTarget::new(&gpu, 90, 60);
    draw_test_scene(&gpu, &frame);
    let source = frame.read_back(&gpu).unwrap();

    // Two columns too narrow and two rows too tall: one column cut on each
    // side, one empty row above and below
    let screen = present(&gpu, &frame, (268, 182), 3);
    for x in 0..268 {
        assert_eq!(screen.pixel(x, 0), [0; 4]);
        assert_eq!(screen.pixel(x, 181), [0; 4]);
        for y in 1..181 {
            assert_eq!(
                screen.pixel(x, y),
                source.pixel((x + 1) / 3, (y - 1) / 3),
                "pixel ({x}, {y})"
            );
        }
    }
}
