use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Stdio};

use isotrail_testkit::diorama_bundle;

fn png_size(path: &Path) -> (u32, u32) {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).unwrap()));
    let info = decoder.read_info().unwrap();
    (info.info().width, info.info().height)
}

#[test]
fn style_lab_renders_every_combination_and_a_contact_sheet() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("hill.itb"), diorama_bundle().to_bytes()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(["style-lab", "hill.itb", "-o", "lab"])
        .current_dir(temp.path())
        .env("ISOTRAIL_CACHE_DIR", temp.path().join("cache"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let lab = temp.path().join("lab");
    let mut images = Vec::new();
    for palette in ["apollo", "resurrect-64", "aap-64"] {
        for mesher in ["voxel", "smooth"] {
            for scale in [3, 5, 6] {
                let name = format!("{mesher}-{scale}-{palette}.png");
                let path = lab.join(&name);
                assert_eq!(png_size(&path), (1080, 1920), "{name}");
                assert!(stdout.contains(&name), "{stdout}");
                images.push(fs::read(&path).unwrap());
            }
        }
    }
    for (i, a) in images.iter().enumerate() {
        for b in &images[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert_eq!(fs::read_dir(&lab).unwrap().count(), 19);
    let (width, height) = png_size(&lab.join("contact-sheet.png"));
    assert!(width > 6 * 1080 && width < 7 * 1080, "{width}");
    assert!(height > 3 * 1920 && height < 4 * 1920, "{height}");
}
