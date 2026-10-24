use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

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

fn filled_cache() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("a.bin"), [0; 1500]).unwrap();
    fs::write(temp.path().join("a.json"), "{}").unwrap();
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
