use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_render::Theme;
use isotrail_testkit::hike_bundle;

fn isotrail(current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .current_dir(current_dir)
        .env("ISOTRAIL_CACHE_DIR", current_dir.join("cache"))
        .env("ISOTRAIL_CONFIG", "")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn loud() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/themes/loud.toml")
}

/// A temporary directory holding the synthetic hike as `hike.itb`
fn with_hike() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("hike.itb"),
        hike_bundle().to_bytes().unwrap(),
    )
    .unwrap();
    temp
}

const SMALL: [&str; 4] = ["--aspect", "square", "--scale", "10"];

fn render(dir: &Path, output: &str, extra: &[&str]) -> Output {
    let mut args = vec!["render", "hike.itb", "-o", output];
    args.extend(SMALL);
    args.extend(extra);
    isotrail(dir, &args)
}

#[test]
fn theme_print_writes_the_built_in_theme() {
    let temp = tempfile::tempdir().unwrap();
    let printed = isotrail(temp.path(), &["-q", "theme", "print"]);
    assert!(printed.status.success(), "{}", text(&printed.stderr));
    let stdout = text(&printed.stdout);
    assert_eq!(Theme::parse(&stdout), Ok(Theme::default()));

    let written = isotrail(temp.path(), &["theme", "print", "-o", "themes/mine.toml"]);
    assert!(written.status.success(), "{}", text(&written.stderr));
    assert!(text(&written.stdout).contains("mine.toml"));
    assert_eq!(
        fs::read_to_string(temp.path().join("themes/mine.toml")).unwrap(),
        stdout
    );
}

#[test]
fn the_built_in_theme_from_a_file_changes_nothing_and_a_loud_one_does() {
    let temp = with_hike();
    let dir = temp.path();
    let theme = isotrail(dir, &["theme", "print", "-o", "built-in.toml"]);
    assert!(theme.status.success(), "{}", text(&theme.stderr));
    for (output, extra) in [
        ("plain.png", vec![]),
        ("built-in.png", vec!["--theme", "built-in.toml"]),
        ("loud.png", vec!["--theme", loud().to_str().unwrap()]),
    ] {
        let rendered = render(dir, output, &extra);
        assert!(rendered.status.success(), "{}", text(&rendered.stderr));
    }
    let read = |name: &str| fs::read(dir.join(name)).unwrap();
    assert_eq!(read("built-in.png"), read("plain.png"));
    assert_ne!(read("loud.png"), read("plain.png"));
}

#[test]
fn theme_mistakes_name_the_file_and_key_before_any_work() {
    let temp = with_hike();
    let dir = temp.path();
    fs::write(dir.join("bad.toml"), "[water]\nglow = \"#ffffff\"\n").unwrap();
    let output = render(dir, "out.png", &["--theme", "bad.toml"]);
    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    let message = text(&output.stderr);
    assert!(
        message.contains("bad.toml") && message.contains("\"glow\"") && message.contains("[water]"),
        "{message}"
    );
    assert!(!dir.join("out.png").exists());

    let utf16: Vec<u8> = [0xff, 0xfe]
        .into_iter()
        .chain("[route]\n".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    fs::write(dir.join("wide.toml"), utf16).unwrap();
    let output = render(dir, "out.png", &["--theme", "wide.toml"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output.stderr).contains("UTF-16"),
        "{}",
        text(&output.stderr)
    );

    let output = render(dir, "out.png", &["--theme", "missing.toml"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("missing.toml"));
}

#[test]
fn config_files_name_a_theme() {
    let temp = with_hike();
    let dir = temp.path();
    fs::write(dir.join("bad.toml"), "[route]\nwidth = 9\n").unwrap();
    fs::write(
        dir.join("settings.toml"),
        "[render]\ntheme = \"bad.toml\"\n",
    )
    .unwrap();
    let output = render(dir, "out.png", &["--config", "settings.toml"]);
    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    let message = text(&output.stderr);
    assert!(
        message.contains("bad.toml") && message.contains("route.width"),
        "{message}"
    );

    fs::write(
        dir.join("settings.toml"),
        "[style-lab]\ntheme = \"bad.toml\"\n",
    )
    .unwrap();
    let output = isotrail(
        dir,
        &[
            "style-lab",
            "hike.itb",
            "-o",
            "lab",
            "--config",
            "settings.toml",
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("route.width"));
}
