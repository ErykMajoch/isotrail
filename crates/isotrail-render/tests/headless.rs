use isotrail_render::{AdapterMode, Gpu, OffscreenTarget, RenderError, draw_test_scene};

const CLEAR: [u8; 4] = [32, 48, 80, 255];
const QUAD: [u8; 4] = [240, 200, 64, 255];

#[test]
fn readback_strips_row_padding() {
    let gpu = Gpu::headless().unwrap();
    // 90 px rows are 360 bytes, padded to 512 for the copy
    let target = OffscreenTarget::new(&gpu, 90, 60);
    draw_test_scene(&gpu, &target);
    let frame = target.read_back(&gpu).unwrap();

    assert_eq!((frame.width(), frame.height()), (90, 60));
    assert_eq!(frame.rgba().len(), 90 * 60 * 4);
    for (x, y) in [(0, 0), (89, 0), (0, 59), (89, 59), (45, 30)] {
        assert_eq!(frame.pixel(x, y), CLEAR, "pixel ({x}, {y})");
    }
    assert_eq!(frame.pixel(65, 30), QUAD);
    let [r, g, b, a] = frame.pixel(21, 30);
    assert!(
        r > 0 && g > 0 && b > 0 && a == 255,
        "triangle interior {r} {g} {b} {a}"
    );
}

#[test]
fn adapter_mode_parses_env_values() {
    assert_eq!("".parse::<AdapterMode>().unwrap(), AdapterMode::Auto);
    assert_eq!(
        " Software ".parse::<AdapterMode>().unwrap(),
        AdapterMode::Software
    );
    assert_eq!(
        "hardware".parse::<AdapterMode>().unwrap(),
        AdapterMode::Hardware
    );
    assert!(matches!(
        "gpu".parse::<AdapterMode>(),
        Err(RenderError::UnknownAdapterMode(value)) if value == "gpu"
    ));
}
