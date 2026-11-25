use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
pub use isotrail_options::theme::ThemeFileArg;
use isotrail_render::Theme;

use crate::output;
use crate::report::say;

#[derive(Debug, Args)]
pub struct ThemeArgs {
    #[command(subcommand)]
    command: ThemeCommand,
}

#[derive(Debug, Subcommand)]
enum ThemeCommand {
    /// Print the built-in theme, with every key at its built-in value
    Print {
        /// Write it to this file instead of printing it
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
            say!("Wrote {}", crate::report::shown(path));
        }
        None => print!("{text}"),
    }
    Ok(())
}
