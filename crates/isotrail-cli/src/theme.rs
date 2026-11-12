use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use isotrail_render::Theme;

use crate::output;
use crate::report::say;
use crate::usage;

/// A theme file for `render`, `embed` and `style-lab`
#[derive(Debug, Args)]
pub struct ThemeFileArg {
    /// Colours, words and fine-tuning from a theme file (TOML); flags and
    /// settings win over it, and isotrail theme print writes one to start from
    #[arg(long, value_name = "FILE")]
    theme: Option<PathBuf>,
}

impl ThemeFileArg {
    pub fn path(&self) -> Option<&Path> {
        self.theme.as_deref()
    }

    /// The theme file read and checked, or the built-in theme without one
    pub fn read(&self) -> anyhow::Result<Theme> {
        match &self.theme {
            Some(path) => read(path),
            None => Ok(Theme::default()),
        }
    }
}

pub fn read(path: &Path) -> anyhow::Result<Theme> {
    let text = crate::read_text(path, "the theme ").map_err(usage)?;
    Theme::parse(&text).map_err(|error| usage(format!("the theme {}: {error}", path.display())))
}

#[derive(Debug, Args)]
pub struct ThemeArgs {
    #[command(subcommand)]
    command: ThemeCommand,
}

#[derive(Debug, Subcommand)]
enum ThemeCommand {
    /// Print the built-in theme, with every key at its built-in value, to
    /// start a theme of your own from
    Print {
        /// Write it to this file instead
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
}

pub fn run(args: &ThemeArgs) -> anyhow::Result<()> {
    let ThemeCommand::Print { output } = &args.command;
    let text = Theme::default().to_toml();
    match output {
        Some(path) => {
            output::plan([path.as_path()], &[])?;
            output::write(path, |file| Ok(file.write_all(text.as_bytes())?))?;
            say!("Wrote {}", path.display());
        }
        None => print!("{text}"),
    }
    Ok(())
}
