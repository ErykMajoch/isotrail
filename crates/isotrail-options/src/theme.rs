use std::path::{Path, PathBuf};

use clap::Args;
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
    let text = crate::read_text(path, "the theme ").map_err(crate::usage)?;
    theme_from_text(&path.display().to_string(), &text)
}

/// A theme file's contents read and checked, with `name` for the file in the
/// message
pub fn theme_from_text(name: &str, text: &str) -> anyhow::Result<Theme> {
    Theme::parse(text).map_err(|error| crate::usage(format!("the theme {name}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_s_text_gets_the_file_s_message() {
        let dir = std::env::temp_dir().join(format!("isotrail-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.toml");
        let text = "[route]\nshade = 1\n";
        std::fs::write(&path, text).unwrap();
        let from_file = read(&path).unwrap_err();
        let from_text = theme_from_text(&path.display().to_string(), text);
        std::fs::remove_dir_all(&dir).unwrap();
        let from_text = from_text.unwrap_err();
        assert_eq!(from_text.to_string(), from_file.to_string());
        assert!(
            from_text
                .to_string()
                .ends_with(r#"unknown key "shade" in the [route] table"#),
            "{from_text}"
        );
        assert!(from_text.is::<crate::UsageError>());
        assert!(theme_from_text("good.toml", "[route]\n").is_ok());
    }
}
