use std::path::{Path, PathBuf};

use clap::Args;
#[cfg(not(target_arch = "wasm32"))]
use isotrail_render::Theme;

/// A theme file for `render`, `embed` and `style-lab`
#[derive(Debug, Args)]
pub struct ThemeFileArg {
    /// Colours, words and fine-tuning from a theme file; isotrail theme print writes one
    #[arg(long, value_name = "FILE", help_heading = crate::LOOK)]
    theme: Option<PathBuf>,
}

impl ThemeFileArg {
    pub fn path(&self) -> Option<&Path> {
        self.theme.as_deref()
    }

    /// The theme file read and checked, or the built-in theme without one
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(&self) -> anyhow::Result<Theme> {
        match &self.theme {
            Some(path) => read(path),
            None => Ok(Theme::default()),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read(path: &Path) -> anyhow::Result<Theme> {
    use crate::usage;
    let text = crate::read_text(path, "the theme ").map_err(usage)?;
    Theme::parse(&text).map_err(|error| usage(format!("the theme {}: {error}", path.display())))
}
