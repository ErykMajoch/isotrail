use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_testkit::hike_bundle;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Runs in `dir` with an empty cache there, and `dir/user-config.toml` as
/// the user config file when it exists
fn isotrail(dir: &Path, args: &[&str]) -> Output {
    let user = dir.join("user-config.toml");
    let mut command = Command::new(env!("CARGO_BIN_EXE_isotrail"));
    command
        .args(args)
        .current_dir(dir)
        .env("ISOTRAIL_CACHE_DIR", dir.join("cache"))
        .env(
            "ISOTRAIL_CONFIG",
            if user.is_file() { user } else { PathBuf::new() },
        )
        .env_remove("ISOTRAIL_OVERPASS_URL")
        .stdin(Stdio::null());
    command.output().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Exit code 2 with clap's usage hint, and `message` in the error
fn assert_usage(output: &Output, command: &str, message: &str) {
    let stderr = stderr(output);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(message), "{stderr}");
    assert!(
        stderr.contains(&format!("Usage: isotrail {command}")),
        "{stderr}"
    );
}

fn assert_ok(output: &Output) {
    assert!(output.status.success(), "{}", stderr(output));
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A temporary directory holding a copy of the Snowdon recording as `walk.gpx`
fn with_walk() -> (tempfile::TempDir, Vec<u8>) {
    let temp = tempfile::tempdir().unwrap();
    let bytes = fs::read(fixtures().join("gpx/snowdon.gpx")).unwrap();
    fs::write(temp.path().join("walk.gpx"), &bytes).unwrap();
    (temp, bytes)
}

#[test]
fn bundle_never_overwrites_its_recording() {
    let (temp, bytes) = with_walk();
    let mut outputs = vec!["walk.gpx", "./walk.gpx"];
    if cfg!(windows) {
        outputs.push("WALK.GPX");
    }
    for output in outputs {
        let run = isotrail(
            temp.path(),
            &["bundle", "walk.gpx", "-o", output, "--offline"],
        );
        assert_usage(&run, "bundle", "one of the files being read");
        assert_eq!(fs::read(temp.path().join("walk.gpx")).unwrap(), bytes);
    }
    let debug = temp.path().join("debug");
    fs::create_dir(&debug).unwrap();
    let run = isotrail(
        temp.path(),
        &["bundle", "walk.gpx", "-o", "debug", "--offline"],
    );
    assert_usage(&run, "bundle", "it is a folder");
}

#[test]
fn render_never_overwrites_its_input_and_makes_missing_folders() {
    let temp = tempfile::tempdir().unwrap();
    let bytes = hike_bundle().to_bytes().unwrap();
    // A bundle is known by its first bytes, whatever its name
    fs::write(temp.path().join("hike.png"), &bytes).unwrap();
    let run = isotrail(temp.path(), &["render", "hike.png", "-o", "hike.png"]);
    assert_usage(&run, "render", "one of the files being read");
    assert_eq!(fs::read(temp.path().join("hike.png")).unwrap(), bytes);

    let run = isotrail(
        temp.path(),
        &["render", "hike.png", "-o", "a/b/still.png", "--scale", "10"],
    );
    assert_ok(&run);
    assert!(temp.path().join("a/b/still.png").is_file());
    assert_eq!(names(&temp.path().join("a/b")), ["still.png"]);
}

#[test]
fn embed_keeps_a_bundle_already_in_place_unless_flags_would_change_it() {
    let temp = tempfile::tempdir().unwrap();
    let bytes = hike_bundle().to_bytes().unwrap();
    fs::write(temp.path().join("hike.itb"), &bytes).unwrap();

    let run = isotrail(
        temp.path(),
        &["embed", "hike.itb", "-o", ".", "--season", "winter"],
    );
    assert_usage(&run, "embed", "write to another folder");
    assert_eq!(fs::read(temp.path().join("hike.itb")).unwrap(), bytes);
    assert_eq!(names(temp.path()), ["hike.itb"]);

    let run = isotrail(temp.path(), &["embed", "hike.itb", "-o", "."]);
    assert_ok(&run);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("Kept"), "{stdout}");
    assert_eq!(fs::read(temp.path().join("hike.itb")).unwrap(), bytes);
    assert_eq!(names(temp.path()), ["hike.itb", "hike.png", "snippet.html"]);

    let run = isotrail(temp.path(), &["embed", "hike.itb", "-o", "hike.png"]);
    assert_usage(&run, "embed", "not a folder");
}

#[test]
fn embed_refuses_to_copy_the_runtime_onto_itself() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("hike.itb"),
        hike_bundle().to_bytes().unwrap(),
    )
    .unwrap();
    let site = temp.path().join("site");
    fs::create_dir_all(site.join("pkg")).unwrap();
    for file in [
        "isotrail-embed.js",
        "pkg/isotrail_web.js",
        "pkg/isotrail_web_bg.wasm",
    ] {
        fs::write(site.join(file), "runtime").unwrap();
    }
    let run = isotrail(
        temp.path(),
        &[
            "embed",
            "hike.itb",
            "-o",
            "site",
            "--with-runtime",
            "--runtime-dir",
            "./site",
        ],
    );
    assert_usage(&run, "embed", "is the folder being written to");
    assert_eq!(names(&site), ["isotrail-embed.js", "pkg"]);
    assert_eq!(
        fs::read_to_string(site.join("pkg/isotrail_web.js")).unwrap(),
        "runtime"
    );
}

#[test]
fn mistakes_are_reported_before_any_download() {
    let (temp, _) = with_walk();
    let untimed = fixtures().join("gpx/no-time.gpx");
    let untimed = untimed.to_str().unwrap();
    let palette: String = (0..256)
        .map(|i| format!("{i:02x}40{:02x}\n", 255 - i))
        .collect();
    fs::write(temp.path().join("big.hex"), palette).unwrap();
    let cases: [(&[&str], &str); 6] = [
        (
            &["--overlay", "none,title"],
            "--overlay none can't be combined",
        ),
        (&["--stats", "gain,time,gain"], "--stats lists gain twice"),
        (
            &["-o", "walk.gif", "--palette", "big.hex"],
            "at most 255 colours",
        ),
        (
            &["-o", "walk.gif", "--palette", "none"],
            "GIF output needs a palette",
        ),
        (
            &["-o", "walk.gif", "--fps", "20", "--step-fps", "25"],
            "--step-fps 25 is more than --fps 20",
        ),
        (&["--aspect", "feed,feed"], "--aspect lists feed twice"),
    ];
    for (flags, message) in cases {
        let mut args = vec!["render", "walk.gpx", "--offline"];
        args.extend_from_slice(flags);
        let run = isotrail(temp.path(), &args);
        assert_usage(&run, "render", message);
        assert!(!stderr(&run).contains("offline"), "{}", stderr(&run));
    }
    let run = isotrail(
        temp.path(),
        &["render", untimed, "--offline", "--time", "07:30"],
    );
    assert_usage(&run, "render", "--time needs a track with times");
    assert_eq!(names(temp.path()), ["big.hex", "walk.gpx"]);
}

#[test]
fn data_flags_typed_for_a_bundle_are_warned_about() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("hike.itb"),
        hike_bundle().to_bytes().unwrap(),
    )
    .unwrap();
    let render = |extra: &[&str]| {
        // No overlays, which give way at this scale with a warning of their own
        let mut args = vec!["render", "hike.itb", "--scale", "10", "--overlay", "none"];
        args.extend_from_slice(extra);
        isotrail(temp.path(), &args)
    };
    let run = render(&[
        "--detail",
        "64",
        "--offline",
        "--padding",
        "10",
        "--footprint",
        "north",
    ]);
    assert_ok(&run);
    let warnings = stderr(&run);
    assert!(warnings.contains("--padding does nothing"), "{warnings}");
    assert!(warnings.contains("--footprint does nothing"), "{warnings}");
    assert!(
        warnings.contains("--detail does nothing with a scene bundle"),
        "{warnings}"
    );
    assert!(warnings.contains("--offline does nothing"), "{warnings}");

    fs::write(
        temp.path().join("user-config.toml"),
        "[render]\ndetail = 64\noffline = true\n",
    )
    .unwrap();
    let run = render(&[]);
    assert_ok(&run);
    assert!(run.stderr.is_empty(), "{}", stderr(&run));
}

#[test]
fn typed_flags_beat_config_settings_they_conflict_with() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("hike.itb"),
        hike_bundle().to_bytes().unwrap(),
    )
    .unwrap();
    fs::write(
        temp.path().join("user-config.toml"),
        "[render]\ndate-format = \"short\"\ntime = 07:30:00\nscale = 10\n",
    )
    .unwrap();
    let run = isotrail(temp.path(), &["render", "hike.itb", "--date", "June"]);
    assert_ok(&run);
    assert!(temp.path().join("hike.png").is_file());

    let run = Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(["render", "hike.itb"])
        .current_dir(temp.path())
        .env("ISOTRAIL_CACHE_DIR", temp.path().join("cache"))
        .env("ISOTRAIL_CONFIG", temp.path().join("missing.toml"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_usage(&run, "render", "ISOTRAIL_CONFIG names");
    assert!(stderr(&run).contains("missing.toml"), "{}", stderr(&run));
}
