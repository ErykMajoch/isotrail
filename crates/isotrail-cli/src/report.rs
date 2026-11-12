use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Args;
use serde::Serialize;

static QUIET: AtomicBool = AtomicBool::new(false);
static JSON: AtomicBool = AtomicBool::new(false);

pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

/// Prints a line for people, unless the run is quiet or reports in JSON
pub fn print_line(line: fmt::Arguments) {
    if !QUIET.load(Ordering::Relaxed) && !JSON.load(Ordering::Relaxed) {
        println!("{line}");
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
    /// Print one JSON object listing the files written, their sizes and the
    /// credit for the caption, instead of the usual lines
    #[arg(long)]
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
            path: path.display().to_string(),
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
