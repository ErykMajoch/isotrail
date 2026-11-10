use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Output, Stdio};

use isotrail_bundle::SceneBundle;
use isotrail_testkit::hike_bundle;

fn isotrail(current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .current_dir(current_dir)
        .env("ISOTRAIL_CACHE_DIR", current_dir.join("cache"))
        .env("ISOTRAIL_CONFIG", current_dir.join("user-config.toml"))
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn png_size(path: &Path) -> (u32, u32) {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).unwrap()));
    let info = decoder.read_info().unwrap();
    (info.info().width, info.info().height)
}

/// A temporary directory holding the synthetic hike as `name`
fn with_hike(name: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join(name), hike_bundle().to_bytes()).unwrap();
    temp
}

fn fake_runtime(dir: &Path) {
    fs::create_dir_all(dir.join("pkg")).unwrap();
    fs::write(dir.join("isotrail-embed.js"), "// element").unwrap();
    fs::write(dir.join("pkg/isotrail_web.js"), "// glue").unwrap();
    fs::write(dir.join("pkg/isotrail_web_bg.wasm"), b"\0asm").unwrap();
}

#[test]
fn writes_a_bundle_poster_and_snippet() {
    let temp = with_hike("ridge.itb");
    let output = isotrail(temp.path(), &["embed", "ridge.itb", "-o", "site"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let site = temp.path().join("site");

    let bundle = SceneBundle::from_bytes(&fs::read(site.join("ridge.itb")).unwrap()).unwrap();
    assert_eq!(bundle.meta.title, hike_bundle().meta.title);
    assert_eq!(png_size(&site.join("ridge.png")), (1920, 1080));
    assert_eq!(
        fs::read_to_string(site.join("snippet.html")).unwrap(),
        "<script type=\"module\" src=\"isotrail-embed.js\"></script>\n\
         <isotrail-hike src=\"ridge.itb\" poster=\"ridge.png\" aspect=\"wide\"></isotrail-hike>\n"
    );
    assert!(!site.join("isotrail-embed.js").exists());
    assert!(!site.join("pkg").exists());
    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("<isotrail-hike src=\"ridge.itb\""),
        "{stdout}"
    );
}

#[test]
fn copies_the_runtime_and_writes_urls_under_the_base() {
    let temp = with_hike("Łąka 2.itb");
    fake_runtime(&temp.path().join("runtime"));
    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "Łąka 2.itb",
            "-o",
            "site/hikes",
            "--aspect",
            "story",
            "--base-url",
            "/hikes",
            "--with-runtime",
            "--runtime-dir",
            "runtime",
            "--overlay",
            "none",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let site = temp.path().join("site/hikes");

    assert_eq!(png_size(&site.join("Łąka 2.png")), (1080, 1920));
    assert!(site.join("Łąka 2.itb").is_file());
    assert_eq!(
        fs::read_to_string(site.join("isotrail-embed.js")).unwrap(),
        "// element"
    );
    assert_eq!(
        fs::read_to_string(site.join("pkg/isotrail_web.js")).unwrap(),
        "// glue"
    );
    assert_eq!(
        fs::read(site.join("pkg/isotrail_web_bg.wasm")).unwrap(),
        b"\0asm"
    );
    let html = fs::read_to_string(site.join("snippet.html")).unwrap();
    assert!(
        html.contains(r#"<script type="module" src="/hikes/isotrail-embed.js">"#),
        "{html}"
    );
    assert!(
        html.contains(r#"src="/hikes/%C5%81%C4%85ka%202.itb""#),
        "{html}"
    );
    assert!(
        html.contains(r#"poster="/hikes/%C5%81%C4%85ka%202.png""#),
        "{html}"
    );
    assert!(html.contains(r#"aspect="story""#), "{html}");
    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("Credit for the caption: © Synthetic"),
        "{stdout}"
    );
}

#[test]
fn the_script_can_point_at_a_shared_runtime() {
    let temp = with_hike("ridge.itb");
    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "ridge.itb",
            "-o",
            "site",
            "--aspect",
            "square",
            "--runtime-url",
            "/isotrail/isotrail-embed.js",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let site = temp.path().join("site");
    assert_eq!(png_size(&site.join("ridge.png")), (1080, 1080));
    let html = fs::read_to_string(site.join("snippet.html")).unwrap();
    assert!(
        html.contains(r#"<script type="module" src="/isotrail/isotrail-embed.js">"#),
        "{html}"
    );
    assert!(html.contains(r#"src="ridge.itb""#), "{html}");
}

#[test]
fn a_missing_runtime_is_refused_before_any_work() {
    let temp = with_hike("ridge.itb");
    let runtime = temp.path().join("runtime");
    fake_runtime(&runtime);
    fs::remove_file(runtime.join("pkg/isotrail_web_bg.wasm")).unwrap();
    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "ridge.itb",
            "-o",
            "site",
            "--with-runtime",
            "--runtime-dir",
            "runtime",
        ],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("isotrail_web_bg.wasm"), "{stderr}");
    assert!(stderr.contains("--runtime-dir"), "{stderr}");
    assert!(!temp.path().join("site").exists());

    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "ridge.itb",
            "-o",
            "site",
            "--runtime-dir",
            "runtime",
        ],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("--with-runtime"), "{stderr}");
}
