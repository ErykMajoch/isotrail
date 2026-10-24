//! Test helpers for isotrail: golden-image comparison with a tolerance
//!
//! Goldens live in `fixtures/golden/<name>.png`. Run the tests with
//! `UPDATE_GOLDENS=1` to create or refresh them. When a comparison fails, the
//! actual image and a diff (differing pixels in red) are written to
//! `target/golden-failures/`

use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

pub const UPDATE_ENV: &str = "UPDATE_GOLDENS";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    /// Largest share of pixels allowed to differ, `0.005` being 0.5%
    pub max_diff_fraction: f64,
    /// Largest per-channel difference at which two pixels still count as equal
    pub channel_epsilon: u8,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            max_diff_fraction: 0.005,
            channel_epsilon: 2,
        }
    }
}

/// Compares an RGBA8 image with `fixtures/golden/<name>.png` using the default [`Tolerance`]
///
/// # Panics
///
/// If the golden is missing, has another size, or too many pixels differ
#[track_caller]
pub fn assert_golden(name: &str, width: u32, height: u32, rgba: &[u8]) {
    assert_golden_with(name, width, height, rgba, Tolerance::default());
}

#[track_caller]
pub fn assert_golden_with(name: &str, width: u32, height: u32, rgba: &[u8], tolerance: Tolerance) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let paths = Paths {
        golden: root.join("fixtures/golden").join(format!("{name}.png")),
        failures: root.join("target/golden-failures"),
        name,
    };
    let image = Image {
        width,
        height,
        rgba,
    };
    if let Err(message) = check(&paths, &image, tolerance, update_requested()) {
        panic!("{message}");
    }
}

fn update_requested() -> bool {
    std::env::var(UPDATE_ENV).is_ok_and(|value| !value.is_empty() && value != "0")
}

struct Paths<'a> {
    golden: PathBuf,
    failures: PathBuf,
    name: &'a str,
}

struct Image<'a> {
    width: u32,
    height: u32,
    rgba: &'a [u8],
}

fn check(paths: &Paths, actual: &Image, tolerance: Tolerance, update: bool) -> Result<(), String> {
    assert_eq!(
        actual.rgba.len(),
        actual.width as usize * actual.height as usize * 4,
        "RGBA buffer doesn't match {} × {}",
        actual.width,
        actual.height
    );
    if update {
        write_png(&paths.golden, actual);
        return Ok(());
    }

    let Some((width, height, expected)) = read_png(&paths.golden) else {
        return Err(format!(
            "golden {} is missing; run with {UPDATE_ENV}=1 to create it",
            paths.golden.display()
        ));
    };
    if (width, height) != (actual.width, actual.height) {
        let actual_path = write_failure(paths, "actual", actual);
        return Err(format!(
            "golden {} is {width} × {height} but the image is {} × {}; actual written to {}",
            paths.name,
            actual.width,
            actual.height,
            actual_path.display()
        ));
    }

    let differs = diff_mask(&expected, actual.rgba, tolerance.channel_epsilon);
    let differing = differs.iter().filter(|&&d| d).count();
    let fraction = differing as f64 / differs.len().max(1) as f64;
    if fraction <= tolerance.max_diff_fraction {
        return Ok(());
    }

    let actual_path = write_failure(paths, "actual", actual);
    let diff = diff_image(actual.rgba, &differs);
    let diff_path = write_failure(
        paths,
        "diff",
        &Image {
            rgba: &diff,
            ..*actual
        },
    );
    Err(format!(
        "golden {} differs in {differing} of {} pixels ({:.3}%, allowed {:.3}%); actual written to {}, diff to {}",
        paths.name,
        differs.len(),
        fraction * 100.0,
        tolerance.max_diff_fraction * 100.0,
        actual_path.display(),
        diff_path.display()
    ))
}

fn diff_mask(expected: &[u8], actual: &[u8], epsilon: u8) -> Vec<bool> {
    let (expected, _) = expected.as_chunks::<4>();
    let (actual, _) = actual.as_chunks::<4>();
    expected
        .iter()
        .zip(actual)
        .map(|(e, a)| e.iter().zip(a).any(|(e, a)| e.abs_diff(*a) > epsilon))
        .collect()
}

fn diff_image(actual: &[u8], differs: &[bool]) -> Vec<u8> {
    actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(differs)
        .flat_map(|(pixel, &differs)| {
            if differs {
                [255, 0, 0, 255]
            } else {
                [pixel[0] / 3, pixel[1] / 3, pixel[2] / 3, 255]
            }
        })
        .collect()
}

fn write_failure(paths: &Paths, kind: &str, image: &Image) -> PathBuf {
    let path = paths.failures.join(format!("{}.{kind}.png", paths.name));
    write_png(&path, image);
    path
}

fn read_png(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let file = File::open(path).ok()?;
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_transformations(png::Transformations::ALPHA | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .unwrap_or_else(|e| panic!("could not read golden {}: {e}", path.display()));
    let mut rgba = vec![0; reader.output_buffer_size().expect("golden too large")];
    let info = reader
        .next_frame(&mut rgba)
        .unwrap_or_else(|e| panic!("could not decode golden {}: {e}", path.display()));
    assert_eq!(
        info.color_type,
        png::ColorType::Rgba,
        "golden {} must be RGBA or convertible to it",
        path.display()
    );
    rgba.truncate(info.buffer_size());
    Some((info.width, info.height, rgba))
}

fn write_png(path: &Path, image: &Image) {
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut encoder = png::Encoder::new(
            BufWriter::new(File::create(path)?),
            image.width,
            image.height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(image.rgba)?;
        writer.finish()?;
        Ok(())
    };
    if let Err(e) = write() {
        panic!("could not write {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 40;
    const H: u32 = 25;

    fn grey(value: u8) -> Vec<u8> {
        [value, value, value, 255].repeat((W * H) as usize)
    }

    fn image(rgba: &[u8]) -> Image<'_> {
        Image {
            width: W,
            height: H,
            rgba,
        }
    }

    fn with_changed_pixels(base: &[u8], count: usize, delta: u8) -> Vec<u8> {
        let mut rgba = base.to_vec();
        for pixel in rgba.as_chunks_mut::<4>().0.iter_mut().take(count) {
            pixel[1] = pixel[1].wrapping_add(delta);
        }
        rgba
    }

    struct Setup {
        _dir: tempfile::TempDir,
        paths: Paths<'static>,
    }

    fn setup_with_golden(golden: &[u8]) -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            golden: dir.path().join("golden/scene.png"),
            failures: dir.path().join("failures"),
            name: "scene",
        };
        check(&paths, &image(golden), Tolerance::default(), true).unwrap();
        Setup { _dir: dir, paths }
    }

    #[test]
    fn identical_image_passes() {
        let s = setup_with_golden(&grey(100));
        check(&s.paths, &image(&grey(100)), Tolerance::default(), false).unwrap();
        assert!(!s.paths.failures.exists());
    }

    #[test]
    fn channel_differences_within_epsilon_are_equal() {
        let s = setup_with_golden(&grey(100));
        let everywhere = with_changed_pixels(&grey(100), (W * H) as usize, 2);
        check(&s.paths, &image(&everywhere), Tolerance::default(), false).unwrap();
        let beyond = with_changed_pixels(&grey(100), (W * H) as usize, 3);
        assert!(check(&s.paths, &image(&beyond), Tolerance::default(), false).is_err());
    }

    #[test]
    fn half_a_percent_of_pixels_may_differ() {
        // 1000 pixels: 5 differing is exactly 0.5%
        let s = setup_with_golden(&grey(100));
        let five = with_changed_pixels(&grey(100), 5, 50);
        check(&s.paths, &image(&five), Tolerance::default(), false).unwrap();
        let six = with_changed_pixels(&grey(100), 6, 50);
        let message = check(&s.paths, &image(&six), Tolerance::default(), false).unwrap_err();
        assert!(message.contains("6 of 1000 pixels"), "{message}");
    }

    #[test]
    fn failure_writes_actual_and_diff_images() {
        let s = setup_with_golden(&grey(100));
        let changed = with_changed_pixels(&grey(100), 20, 50);
        check(&s.paths, &image(&changed), Tolerance::default(), false).unwrap_err();

        let (_, _, actual) = read_png(&s.paths.failures.join("scene.actual.png")).unwrap();
        assert_eq!(actual, changed);
        let (_, _, diff) = read_png(&s.paths.failures.join("scene.diff.png")).unwrap();
        assert_eq!(&diff[..4], [255, 0, 0, 255]);
        assert_eq!(&diff[diff.len() - 4..], [33, 33, 33, 255]);
    }

    #[test]
    fn size_mismatch_fails() {
        let s = setup_with_golden(&grey(100));
        let small = [0u8; 4 * 4];
        let other = Image {
            width: 2,
            height: 2,
            rgba: &small,
        };
        let message = check(&s.paths, &other, Tolerance::default(), false).unwrap_err();
        assert!(message.contains("40 × 25"), "{message}");
    }

    #[test]
    fn missing_golden_explains_how_to_create_it() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            golden: dir.path().join("none.png"),
            failures: dir.path().join("failures"),
            name: "none",
        };
        let message = check(&paths, &image(&grey(0)), Tolerance::default(), false).unwrap_err();
        assert!(message.contains("UPDATE_GOLDENS=1"), "{message}");
    }
}
