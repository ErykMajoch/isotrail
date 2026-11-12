use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Args;
use serde::Serialize;

static QUIET: AtomicBool = AtomicBool::new(false);
static JSON: AtomicBool = AtomicBool::new(false);
static DOWNLOADING: AtomicBool = AtomicBool::new(false);

pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

/// Prints a line for people, unless the run is quiet or reports in JSON
pub fn print_line(line: fmt::Arguments) {
    if !QUIET.load(Ordering::Relaxed) && !JSON.load(Ordering::Relaxed) {
        println!("{line}");
    }
}

/// Says once per run, on stderr, that data is being downloaded, unless the
/// run is quiet or reports in JSON
pub fn downloading(line: &str) {
    if !DOWNLOADING.swap(true, Ordering::Relaxed)
        && !QUIET.load(Ordering::Relaxed)
        && !JSON.load(Ordering::Relaxed)
    {
        eprintln!("{line}");
    }
}

/// The line before a run's first download, naming what it fetches
pub fn download_line(map_data: bool, weather: bool) -> String {
    let what = match (map_data, weather) {
        (true, true) => "elevation, map and weather data",
        (true, false) => "elevation and map data",
        (false, true) => "elevation and weather data",
        (false, false) => "elevation data",
    };
    format!("Downloading {what} for this area (kept in the cache for next time)")
}

/// A path as printed, with the separator the user typed: on Windows, a path
/// given with forward slashes keeps them in the parts isotrail adds
pub fn shown(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) && text.contains('/') {
        text.replace('\\', "/")
    } else {
        text
    }
}

macro_rules! say {
    ($($arg:tt)*) => {
        $crate::report::print_line(format_args!($($arg)*))
    };
}
pub(crate) use say;

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// Print one JSON object with the files written and the caption credit, for scripts
    #[arg(long, help_heading = crate::OUTPUT)]
    json: bool,
}

/// A file a command wrote
#[derive(Debug, Serialize)]
pub struct Written {
    path: String,
    bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frames: Option<u32>,
}

impl Written {
    pub fn file(path: &Path) -> Written {
        Written {
            path: shown(path),
            bytes: fs::metadata(path).map_or(0, |meta| meta.len()),
            width: None,
            height: None,
            frames: None,
        }
    }

    pub fn image(path: &Path, (width, height): (u32, u32)) -> Written {
        Written {
            width: Some(width),
            height: Some(height),
            ..Written::file(path)
        }
    }

    pub fn clip(path: &Path, size: (u32, u32), frames: u32) -> Written {
        Written {
            frames: Some(frames),
            ..Written::image(path, size)
        }
    }
}

#[derive(Serialize)]
struct Summary<'a> {
    files: &'a [Written],
    caption_credit: Option<&'a str>,
}

/// What a command wrote, told as lines along the way or as one JSON object
/// at the end
pub struct Report {
    json: bool,
    files: Vec<Written>,
}

impl Report {
    pub fn new(args: &ReportArgs) -> Report {
        JSON.store(args.json, Ordering::Relaxed);
        Report {
            json: args.json,
            files: Vec::new(),
        }
    }

    pub fn wrote(&mut self, written: Written) {
        say!("Wrote {}", written.path);
        self.files.push(written);
    }

    /// A file already told about in a line of its own
    pub fn add(&mut self, written: Written) {
        self.files.push(written);
    }

    /// A file kept as it was, still part of what the command produced
    pub fn kept(&mut self, written: Written) {
        say!("Kept {} as it is", written.path);
        self.files.push(written);
    }

    /// Ends the report, with the credit to paste into a caption when the
    /// image doesn't show it
    pub fn finish(self, caption_credit: Option<&str>) -> anyhow::Result<()> {
        if self.json {
            let summary = Summary {
                files: &self.files,
                caption_credit,
            };
            let mut out = io::stdout().lock();
            serde_json::to_writer_pretty(&mut out, &summary)?;
            writeln!(out)?;
        } else if let Some(credit) = caption_credit {
            say!("Credit for the caption: {credit}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_download_line_names_what_is_fetched() {
        assert_eq!(
            download_line(true, true),
            "Downloading elevation, map and weather data for this area (kept in the cache for next time)"
        );
        assert!(download_line(true, false).starts_with("Downloading elevation and map data "));
        assert!(download_line(false, false).starts_with("Downloading elevation data "));
    }

    #[test]
    fn paths_keep_the_separator_typed() {
        let joined = Path::new("site/hikes/my-hike").join("hike.itb");
        let expected = "site/hikes/my-hike/hike.itb";
        assert_eq!(shown(&joined), expected);
        assert_eq!(shown(Path::new("hike.png")), "hike.png");
        if cfg!(windows) {
            let typed = Path::new(r"C:\hikes").join("hike.itb");
            assert_eq!(shown(&typed), r"C:\hikes\hike.itb");
        }
        assert_eq!(Written::file(&joined).path, expected);
    }
}
