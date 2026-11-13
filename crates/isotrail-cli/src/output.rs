use std::fmt;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use tracing::debug;

use crate::usage;

/// Exit code of a run stopped by Ctrl+C, as shells report it
pub const INTERRUPTED_EXIT: u8 = 130;
pub const STOPPED: &str = "Stopped; nothing was left half-written";

/// Temporary files being written, removed if the run is stopped
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub struct Interrupted;

impl fmt::Display for Interrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("interrupted")
    }
}

impl std::error::Error for Interrupted {}

/// Ctrl+C stops the run at once, unless a file is being written: then the
/// writer stops at its next frame and removes its temporary file. A second
/// Ctrl+C doesn't wait
pub fn catch_interrupts() {
    let handler = || {
        let writing = PENDING.lock().is_ok_and(|pending| !pending.is_empty());
        if writing && !INTERRUPTED.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(pending) = PENDING.lock() {
            for path in pending.iter() {
                let _ = fs::remove_file(path);
            }
        }
        crate::report::failed(crate::report::Reason::Interrupted, STOPPED);
        process::exit(i32::from(INTERRUPTED_EXIT));
    };
    if let Err(error) = ctrlc::set_handler(handler) {
        debug!("could not catch Ctrl+C: {error}");
    }
}

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

pub fn check_interrupted() -> anyhow::Result<()> {
    if interrupted() {
        Err(Interrupted.into())
    } else {
        Ok(())
    }
}

/// Checks every output before any work: none may be a folder or one of the
/// run's inputs. Then creates the folders they go in
pub fn plan<'a>(
    outputs: impl IntoIterator<Item = &'a Path>,
    inputs: &[&Path],
) -> anyhow::Result<()> {
    let outputs: Vec<&Path> = outputs.into_iter().collect();
    for &output in &outputs {
        if output.is_dir() {
            return Err(usage(format!(
                "can't write {}: it is a folder",
                output.display()
            )));
        }
        if inputs.iter().any(|input| same_file(input, output)) {
            return Err(usage(format!(
                "can't write {}: it is one of the files being read; choose another name",
                output.display()
            )));
        }
    }
    for output in outputs {
        if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .with_context(|| format!("can't create the folder {}", parent.display()))?;
        }
    }
    Ok(())
}

/// A folder to write into, created if missing
pub fn folder(dir: &Path) -> anyhow::Result<()> {
    if dir.exists() && !dir.is_dir() {
        return Err(usage(format!(
            "can't write into {}: it is a file, not a folder",
            dir.display()
        )));
    }
    fs::create_dir_all(dir).with_context(|| format!("can't create the folder {}", dir.display()))
}

/// Both paths name one existing file or folder, however they are spelt
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Writes a whole file through [`Pending`]
pub fn write(
    target: &Path,
    contents: impl FnOnce(&mut BufWriter<File>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let pending = Pending::new(target);
    let mut file = pending.open()?;
    contents(&mut file)
        .and_then(|()| Ok(file.flush()?))
        .with_context(|| format!("can't write {}", target.display()))?;
    drop(file);
    pending.commit()
}

/// A file written under a temporary name beside its target, which replaces
/// the target once complete; dropped uncommitted, it is removed
pub struct Pending {
    target: PathBuf,
    temp: PathBuf,
    committed: bool,
}

impl Pending {
    pub fn new(target: &Path) -> Pending {
        let mut name = target.file_name().unwrap_or_default().to_owned();
        name.push(format!(".{}.tmp", process::id()));
        let temp = target.with_file_name(name);
        if let Ok(mut pending) = PENDING.lock() {
            pending.push(temp.clone());
        }
        Pending {
            target: target.to_owned(),
            temp,
            committed: false,
        }
    }

    pub fn temp(&self) -> &Path {
        &self.temp
    }

    pub fn open(&self) -> anyhow::Result<BufWriter<File>> {
        let file = File::create(&self.temp)
            .with_context(|| format!("can't write {}", self.target.display()))?;
        Ok(BufWriter::new(file))
    }

    /// Puts the finished file in place, unless the run was stopped
    pub fn commit(mut self) -> anyhow::Result<()> {
        check_interrupted()?;
        fs::rename(&self.temp, &self.target)
            .with_context(|| format!("can't write {}", self.target.display()))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temp);
        }
        if let Ok(mut pending) = PENDING.lock()
            && let Some(i) = pending.iter().position(|path| *path == self.temp)
        {
            pending.swap_remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::bail;

    use super::*;
    use crate::UsageError;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_finished_file_replaces_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.png");
        fs::write(&target, "old").unwrap();
        write(&target, |w| Ok(w.write_all(b"new")?)).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(names(dir.path()), ["a.png"]);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_temporary_one() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.gif");
        fs::write(&target, "good").unwrap();
        let result = write(&target, |w| {
            w.write_all(&[7; 100_000])?;
            w.flush()?;
            bail!("frame 3 failed")
        });
        let message = format!("{:#}", result.unwrap_err());
        assert!(message.contains("can't write"), "{message}");
        assert!(message.contains("frame 3 failed"), "{message}");
        assert_eq!(fs::read(&target).unwrap(), b"good");
        assert_eq!(names(dir.path()), ["a.gif"]);

        let pending = Pending::new(&target);
        let temp = pending.temp().to_owned();
        let mut file = pending.open().unwrap();
        file.write_all(b"half").unwrap();
        assert_eq!(names(dir.path()).len(), 2);
        drop((file, pending));
        assert_eq!(names(dir.path()), ["a.gif"]);
        assert!(!PENDING.lock().unwrap().contains(&temp));
    }

    #[test]
    fn outputs_may_not_be_inputs_or_folders() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("walk.gpx");
        fs::write(&input, "<gpx/>").unwrap();
        let same = dir.path().join(".").join("walk.gpx");
        let error = plan([same.as_path()], &[&input]).unwrap_err();
        assert!(error.is::<UsageError>());
        assert!(error.to_string().contains("files being read"), "{error}");
        if cfg!(windows) {
            let shouted = dir.path().join("WALK.GPX");
            assert!(plan([shouted.as_path()], &[&input]).is_err());
        }
        let error = plan([dir.path()], &[]).unwrap_err();
        assert!(error.to_string().contains("is a folder"), "{error}");

        let nested = dir.path().join("a/b/walk.png");
        plan([nested.as_path()], &[&input]).unwrap();
        assert!(dir.path().join("a/b").is_dir());
        assert!(!nested.exists());

        let error = folder(&input).unwrap_err();
        assert!(error.to_string().contains("not a folder"), "{error}");
        folder(&dir.path().join("site")).unwrap();
        assert!(dir.path().join("site").is_dir());
    }
}
