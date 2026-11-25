use std::fs;
use std::io;
use std::path::Path;

use anyhow::anyhow;

use crate::plain;

/// What an I/O error means, in words
pub fn io_words(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "the file doesn't exist".into(),
        io::ErrorKind::PermissionDenied => "permission denied".into(),
        io::ErrorKind::IsADirectory => "it is a folder".into(),
        io::ErrorKind::NotADirectory => "part of the path is a file, not a folder".into(),
        io::ErrorKind::StorageFull => "the disk is full".into(),
        io::ErrorKind::ReadOnlyFilesystem => "the disk is read-only".into(),
        io::ErrorKind::InvalidFilename => "the name can't be used for a file here".into(),
        _ => plain(&error.to_string()),
    }
}

/// Why a file couldn't be read or written, in words
pub fn file_problem(path: &Path, error: &io::Error) -> String {
    if path.is_dir() {
        "it is a folder".into()
    } else {
        io_words(error)
    }
}

/// The file's bytes, or one message naming `what` it is and the file, such
/// as `can't open the theme dusk.toml: the file doesn't exist`
pub fn read_named(path: &Path, what: &str) -> anyhow::Result<Vec<u8>> {
    fs::read(path).map_err(|error| {
        anyhow!(
            "can't open {what}{}: {}",
            path.display(),
            file_problem(path, &error)
        )
    })
}

/// A settings file's text, or why it can't be read, naming `what` it is
pub fn read_text(path: &Path, what: &str) -> Result<String, String> {
    let bytes = read_named(path, what).map_err(|error| error.to_string())?;
    let shown = path.display();
    if bytes.starts_with(&[0xff, 0xfe])
        || bytes.starts_with(&[0xfe, 0xff])
        || utf16_unmarked(&bytes)
    {
        return Err(format!(
            "{what}{shown} is saved as UTF-16 (as PowerShell's > writes it); save it as UTF-8"
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("{what}{shown} isn't UTF-8 text"))
}

/// UTF-16 without a byte-order mark: mostly English text puts a zero in
/// every other byte, and UTF-8 text has none
fn utf16_unmarked(bytes: &[u8]) -> bool {
    let start = &bytes[..bytes.len().min(256) & !1];
    let zeros = |parity: usize| {
        start
            .iter()
            .skip(parity)
            .step_by(2)
            .filter(|&&b| b == 0)
            .count()
    };
    let pairs = start.len() / 2;
    let (even, odd) = (zeros(0), zeros(1));
    pairs > 0 && (even.min(odd) == 0) && even.max(odd) * 2 > pairs
}
