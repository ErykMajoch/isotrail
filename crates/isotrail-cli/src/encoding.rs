use std::num::NonZeroU16;
use std::path::PathBuf;

use clap::Args;
use clap::builder::PossibleValuesParser;
use isotrail_export::{DEFAULT_CRF, Ffmpeg, GifOptions, MAX_CRF, Mp4Options, Plays, X264_PRESETS};
use tracing::warn;

use crate::config::Typed;
use crate::usage;

const MAX_PLAYS: u16 = 1000;

/// How an animation is written to its file
#[derive(Debug, Args)]
pub struct EncodingArgs {
    /// Output pixels per drawn pixel in a GIF, from 1 (the drawn size, for
    /// small files on a website) up to --scale, the default
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=16))]
    gif_scale: Option<u32>,

    /// How many times a GIF plays: forever, once, or a number of times
    #[arg(long = "loop", id = "loop", value_name = "forever|once|N", default_value = "forever", value_parser = parse_plays)]
    plays: Plays,

    /// MP4 quality, from 0 (lossless, which many players can't play) to 51;
    /// lower is better and larger
    #[arg(
        long,
        value_name = "N",
        default_value_t = DEFAULT_CRF,
        value_parser = clap::value_parser!(u8).range(0..=i64::from(MAX_CRF))
    )]
    crf: u8,

    /// How hard the MP4 encoder works: slower presets make smaller files at
    /// the same quality; x264's own default (medium) if not given
    #[arg(long, value_name = "NAME", value_parser = PossibleValuesParser::new(X264_PRESETS))]
    preset: Option<String>,

    /// The ffmpeg program to make MP4s with, instead of the one on PATH
    #[arg(long, value_name = "PATH", env = "ISOTRAIL_FFMPEG")]
    ffmpeg: Option<PathBuf>,
}

pub(crate) fn parse_plays(value: &str) -> Result<Plays, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "forever" => Ok(Plays::Forever),
        "once" => Ok(Plays::Times(NonZeroU16::MIN)),
        number => number
            .parse::<u16>()
            .ok()
            .filter(|n| (1..=MAX_PLAYS).contains(n))
            .and_then(NonZeroU16::new)
            .map(Plays::Times)
            .ok_or_else(|| format!("expected forever, once or a number from 1 to {MAX_PLAYS}")),
    }
}

/// The output formats an animation flag applies to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    Gif,
    Mp4,
}

impl EncodingArgs {
    /// Checks the settings against the clip's scale and warns about typed
    /// flags that the format leaves unused; `None` for a still
    pub fn check(&self, clip: Option<Clip>, scale: u32, typed: &Typed) -> anyhow::Result<()> {
        if let Some(gif_scale) = self.gif_scale
            && gif_scale > scale
            && clip == Some(Clip::Gif)
        {
            return Err(usage(format!(
                "--gif-scale {gif_scale} is more than --scale {scale}: a GIF can be made smaller than the image, not larger"
            )));
        }
        let unused = |flags: &[(&str, &str)], what: &str| {
            for (id, flag) in flags {
                if typed.has(id) {
                    warn!("{flag} does nothing here: it only applies to {what}");
                }
            }
        };
        let gif = [("gif_scale", "--gif-scale"), ("loop", "--loop")];
        let mp4 = [
            ("crf", "--crf"),
            ("preset", "--preset"),
            ("ffmpeg", "--ffmpeg"),
        ];
        if clip != Some(Clip::Gif) {
            unused(&gif, "GIF output");
        }
        if clip != Some(Clip::Mp4) {
            unused(&mp4, "MP4 output");
        }
        Ok(())
    }

    pub fn gif(&self, scale: u32, fps: u32) -> GifOptions {
        GifOptions {
            scale: self.gif_scale.unwrap_or(scale).min(scale),
            fps,
            plays: self.plays,
        }
    }

    pub fn mp4(&self, scale: u32, fps: u32) -> Mp4Options {
        Mp4Options {
            scale,
            fps,
            crf: self.crf,
            preset: self
                .preset
                .as_deref()
                .and_then(|name| X264_PRESETS.into_iter().find(|&preset| preset == name)),
        }
    }

    pub fn ffmpeg(&self) -> anyhow::Result<Ffmpeg> {
        Ok(match &self.ffmpeg {
            Some(path) => Ffmpeg::at(path)?,
            None => Ffmpeg::find()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(flatten)]
        encoding: EncodingArgs,
    }

    fn args(flags: &[&str]) -> anyhow::Result<EncodingArgs> {
        let mut line = vec!["isotrail"];
        line.extend(flags);
        Ok(Cli::try_parse_from(line)?.encoding)
    }

    #[test]
    fn plays_are_forever_once_or_a_count() {
        let times = |n| Plays::Times(NonZeroU16::new(n).unwrap());
        assert_eq!(parse_plays("forever"), Ok(Plays::Forever));
        assert_eq!(parse_plays("Once"), Ok(times(1)));
        assert_eq!(parse_plays("1"), Ok(times(1)));
        assert_eq!(parse_plays("3"), Ok(times(3)));
        for bad in ["0", "1001", "-2", "twice", ""] {
            assert!(parse_plays(bad).is_err(), "{bad}");
        }
        assert_eq!(args(&[]).unwrap().plays, Plays::Forever);
    }

    #[test]
    fn gifs_shrink_and_mp4s_take_quality_and_a_preset() {
        let defaults = args(&[]).unwrap();
        assert_eq!(defaults.gif(3, 30).scale, 3);
        assert_eq!(defaults.mp4(3, 30).crf, DEFAULT_CRF);
        assert_eq!(defaults.mp4(3, 30).preset, None);
        let small = args(&["--gif-scale", "1"]).unwrap();
        assert_eq!(small.gif(3, 30).scale, 1);
        let tuned = args(&["--crf", "23", "--preset", "slow"]).unwrap();
        assert_eq!(tuned.mp4(3, 60).crf, 23);
        assert_eq!(tuned.mp4(3, 60).preset, Some("slow"));
        for bad in [
            &["--crf", "52"][..],
            &["--preset", "quick"],
            &["--gif-scale", "0"],
        ] {
            assert!(args(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_gif_is_never_larger_than_the_image() {
        let big = args(&["--gif-scale", "5"]).unwrap();
        let typed = Typed::default();
        let message = big
            .check(Some(Clip::Gif), 3, &typed)
            .unwrap_err()
            .to_string();
        assert!(message.contains("--scale 3"), "{message}");
        assert!(big.check(Some(Clip::Gif), 6, &typed).is_ok());
        assert!(big.check(None, 3, &typed).is_ok());
    }
}
