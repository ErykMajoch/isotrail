use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime};

fn isotrail(cache_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .env("ISOTRAIL_CACHE_DIR", cache_dir)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn key(digit: char) -> String {
    digit.to_string().repeat(64)
}

fn filled_cache() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join(format!("{}.bin", key('a'))), [0; 1500]).unwrap();
    fs::write(temp.path().join(format!("{}.json", key('a'))), "{}").unwrap();
    temp
}

#[test]
fn path_prints_the_cache_directory() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("cache");
    let output = isotrail(&dir, &["cache", "path"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout).trim_end(), dir.display().to_string());
}

#[test]
fn clear_with_yes_deletes_cached_files() {
    let temp = filled_cache();
    let output = isotrail(temp.path(), &["cache", "clear", "--yes"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout).trim_end(), "Deleted 2 files (1.5 kB)");
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn clear_without_a_terminal_needs_yes() {
    let temp = filled_cache();
    let output = isotrail(temp.path(), &["cache", "clear"]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("pass --yes"),
        "{}",
        text(&output.stderr)
    );
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[test]
fn clearing_an_empty_cache_says_so() {
    let temp = tempfile::tempdir().unwrap();
    let output = isotrail(temp.path(), &["cache", "clear"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("is empty"));
}

#[test]
fn clear_leaves_other_files_alone_and_counts_them() {
    let temp = filled_cache();
    for name in ["package.json", "data.bin", "notes.txt"] {
        fs::write(temp.path().join(name), "mine").unwrap();
    }
    let output = isotrail(temp.path(), &["cache", "clear", "--yes"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout).trim_end(),
        "Deleted 2 files (1.5 kB); left 3 other files alone"
    );
    let mut left: Vec<String> = fs::read_dir(temp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["data.bin", "notes.txt", "package.json"]);
}

#[test]
fn clear_older_than_keeps_recent_downloads() {
    let temp = filled_cache();
    let old = temp.path().join(format!("{}.bin", key('b')));
    fs::write(&old, [0; 500]).unwrap();
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(10 * 86_400))
        .unwrap();

    let output = isotrail(
        temp.path(),
        &["cache", "clear", "--yes", "--older-than", "30"],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        text(&output.stdout).starts_with("Nothing in the cache at "),
        "{}",
        text(&output.stdout)
    );
    let output = isotrail(
        temp.path(),
        &["cache", "clear", "--yes", "--older-than", "7"],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout).trim_end(),
        "Deleted 1 file (500 B) downloaded more than 7 days ago"
    );
    assert!(!old.exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[test]
fn a_cache_folder_given_on_the_command_line_wins() {
    let temp = tempfile::tempdir().unwrap();
    let chosen = temp.path().join("chosen");
    let from_env = temp.path().join("from-env");
    for args in [
        ["cache", "path", "--cache-dir", chosen.to_str().unwrap()],
        ["cache", "--cache-dir", chosen.to_str().unwrap(), "path"],
    ] {
        let output = isotrail(&from_env, &args);
        assert!(output.status.success(), "{}", text(&output.stderr));
        assert_eq!(
            text(&output.stdout).trim_end(),
            chosen.display().to_string()
        );
    }
    let cache = filled_cache();
    let output = isotrail(
        &from_env,
        &[
            "cache",
            "clear",
            "--yes",
            "--cache-dir",
            cache.path().to_str().unwrap(),
            "-q",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
}
