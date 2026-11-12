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
        .env("ISOTRAIL_CONFIG", "")
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
    fs::write(temp.path().join(name), hike_bundle().to_bytes().unwrap()).unwrap();
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
        format!(
            "<script type=\"module\" src=\"isotrail-embed.js?v={}\"></script>\n\
             <isotrail-hike src=\"ridge.itb\" poster=\"ridge.png\" aspect=\"wide\" \
             style=\"display:block;aspect-ratio:16/9\"><img src=\"ridge.png\" \
             alt=\"Diorama of {}\" style=\"display:block;width:100%\"></isotrail-hike>\n",
            env!("CARGO_PKG_VERSION"),
            hike_bundle().meta.title
        )
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
        html.contains(r#"<script type="module" src="/hikes/isotrail-embed.js?v="#),
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
        html.contains(r#"<script type="module" src="/isotrail/isotrail-embed.js?v="#),
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

fn isotrail_with_config(current_dir: &Path, args: &[&str], config: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .current_dir(current_dir)
        .env("ISOTRAIL_CACHE_DIR", current_dir.join("cache"))
        .env("ISOTRAIL_CONFIG", config)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn the_poster_and_snippet_carry_the_look() {
    let temp = with_hike("ridge.itb");
    let plain = isotrail(temp.path(), &["embed", "ridge.itb", "-o", "plain"]);
    assert!(plain.status.success(), "{}", text(&plain.stderr));
    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "ridge.itb",
            "-o",
            "look",
            "--palette",
            "resurrect-64",
            "--mesher",
            "smooth",
            "--god-rays",
            "off",
            "--scale",
            "5",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let (plain, look) = (temp.path().join("plain"), temp.path().join("look"));
    assert_eq!(png_size(&look.join("ridge.png")), (1920, 1080));
    assert_ne!(
        fs::read(plain.join("ridge.png")).unwrap(),
        fs::read(look.join("ridge.png")).unwrap()
    );
    let html = fs::read_to_string(look.join("snippet.html")).unwrap();
    assert!(
        html.contains(
            r#"aspect="wide" palette="resurrect-64" mesher="smooth" god-rays="off" style="#
        ),
        "{html}"
    );
    // The bundle itself carries no look
    assert_eq!(
        fs::read(plain.join("ridge.itb")).unwrap(),
        fs::read(look.join("ridge.itb")).unwrap()
    );
}

#[test]
fn the_starting_view_and_size_reach_the_snippet() {
    let temp = with_hike("ridge.itb");
    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "ridge.itb",
            "-o",
            "view",
            "--turn=-30",
            "--pitch",
            "45",
            "--zoom",
            "1.5",
            "--aspect",
            "portrait",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let view = temp.path().join("view");
    assert_eq!(png_size(&view.join("ridge.png")), (1080, 1440));
    let html = fs::read_to_string(view.join("snippet.html")).unwrap();
    assert!(
        html.contains(r#"aspect="portrait" turn="-30" pitch="45" zoom="1.5" style="display:block;aspect-ratio:3/4""#),
        "{html}"
    );

    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "ridge.itb",
            "-o",
            "sized",
            "--size",
            "1200x900",
            "--heading",
            "90",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let sized = temp.path().join("sized");
    assert_eq!(png_size(&sized.join("ridge.png")), (1200, 900));
    let html = fs::read_to_string(sized.join("snippet.html")).unwrap();
    assert!(html.contains(r#"aspect="4:3" turn=""#), "{html}");
    assert!(html.contains("aspect-ratio:4/3"), "{html}");
}

#[test]
fn a_palette_file_is_copied_beside_the_bundle() {
    let temp = with_hike("ridge.itb");
    fs::create_dir(temp.path().join("colours")).unwrap();
    fs::write(
        temp.path().join("colours/warm.hex"),
        "1a1c2c\n5d275d\nb13e53\nef7d57\nffcd75\na7f070\n",
    )
    .unwrap();
    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "ridge.itb",
            "-o",
            "site",
            "--palette",
            "colours/warm.hex",
            "--base-url",
            "/hikes/",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let site = temp.path().join("site");
    assert_eq!(
        fs::read(site.join("warm.hex")).unwrap(),
        fs::read(temp.path().join("colours/warm.hex")).unwrap()
    );
    let html = fs::read_to_string(site.join("snippet.html")).unwrap();
    assert!(html.contains(r#"palette="/hikes/warm.hex""#), "{html}");

    // Embedding again from the copy leaves it where it is
    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "ridge.itb",
            "-o",
            "site",
            "--palette",
            "site/warm.hex",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let html = fs::read_to_string(site.join("snippet.html")).unwrap();
    assert!(html.contains(r#"palette="warm.hex""#), "{html}");
}

#[test]
fn embed_reads_config_files() {
    let temp = with_hike("ridge.itb");
    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        "[render]\npalette = \"aap-64\"\naspect = [\"story\", \"feed\"]\nduration = \"5s\"\n\n\
         [embed]\nmesher = \"smooth\"\n",
    )
    .unwrap();
    let output = isotrail_with_config(temp.path(), &["embed", "ridge.itb", "-o", "site"], &config);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let site = temp.path().join("site");
    let html = fs::read_to_string(site.join("snippet.html")).unwrap();
    assert!(
        html.contains(r#"aspect="wide" palette="aap-64" mesher="smooth" style="#),
        "{html}"
    );
    assert_eq!(png_size(&site.join("ridge.png")), (1920, 1080));

    // A typed flag still wins
    let output = isotrail_with_config(
        temp.path(),
        &["embed", "ridge.itb", "-o", "site", "--palette", "apollo"],
        &config,
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let html = fs::read_to_string(site.join("snippet.html")).unwrap();
    assert!(!html.contains("palette="), "{html}");
}

#[test]
fn the_title_is_saved_in_the_bundle() {
    let temp = with_hike("ridge.itb");
    let output = isotrail(
        temp.path(),
        &["embed", "ridge.itb", "-o", "site", "--title", "Łańcuch"],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let site = temp.path().join("site");
    let bundle = SceneBundle::from_bytes(&fs::read(site.join("ridge.itb")).unwrap()).unwrap();
    assert_eq!(bundle.meta.title, "Łańcuch");
    let html = fs::read_to_string(site.join("snippet.html")).unwrap();
    assert!(html.contains(r#"alt="Diorama of Łańcuch""#), "{html}");

    // In place, the same title keeps the bundle and another is refused
    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "site/ridge.itb",
            "-o",
            "site",
            "--title",
            "Łańcuch",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let output = isotrail(
        temp.path(),
        &["embed", "site/ridge.itb", "-o", "site", "--title", "Other"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output.stderr).contains("the flags would change it"),
        "{}",
        text(&output.stderr)
    );
}
