use clap::{Args, ValueEnum};
use isotrail_export::{MAX_GIF_FPS, MAX_MP4_FPS};
use isotrail_render::{
    Aim, Conditions, DEFAULT_FPS, DEFAULT_ORBIT_DEG, MIN_DRAW_S, Motion, Orbit, Pace, Pacing,
    Timeline, parse_count, parse_within,
};

use crate::usage;

const MAX_GIF_DURATION_S: f32 = 60.0;
const MAX_MP4_DURATION_S: f32 = 180.0;

#[derive(Debug, Args)]
#[command(next_help_heading = crate::ANIMATION)]
pub struct AnimationArgs {
    /// Length in seconds, from 1 to 60 for GIFs and 180 for MP4s
    #[arg(long, value_name = "SECONDS", default_value = "10s", value_parser = parse_duration)]
    duration: f32,

    /// Seconds on the scene before the route draws, from 0 to 180 [default: 5% of the clip]
    #[arg(long, value_name = "SECONDS", value_parser = parse_part)]
    intro: Option<f32>,

    /// Seconds on the finished hike, from 0 to 180 [default: 15% of the clip]
    #[arg(long, value_name = "SECONDS", value_parser = parse_part)]
    hold: Option<f32>,

    /// After the hold, undraw the route and turn back, for a loop without a jump
    #[arg(long)]
    loop_back: bool,

    /// Frames per second, from 1 to 50 for GIFs and 60 for MP4s
    #[arg(
        long,
        value_name = "N",
        default_value_t = DEFAULT_FPS,
        value_parser = parse_fps
    )]
    fps: u32,

    /// Different images per second, from 1 to --fps; fewer gives choppy, game-like motion
    #[arg(long, value_name = "N", value_parser = parse_fps)]
    step_fps: Option<u32>,

    /// How far the camera turns, in degrees from -360 to 360; 0 keeps it still
    #[arg(long, value_name = "DEG", default_value_t = DEFAULT_ORBIT_DEG, value_parser = parse_orbit)]
    orbit: f32,

    /// Draw the route at an even speed, or following the recorded times
    #[arg(long, value_enum, value_name = "distance|time", default_value_t = PaceArg::Distance,
          hide_possible_values = true)]
    pace: PaceArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum PaceArg {
    Distance,
    Time,
}

impl AnimationArgs {
    /// The clip's motion; a GIF has lower limits than an MP4
    pub fn motion(&self, gif: bool) -> anyhow::Result<Motion> {
        if gif && self.duration > MAX_GIF_DURATION_S {
            return Err(usage(format!(
                "a GIF can be at most 60s long, not {}s; an MP4 can be up to 180s",
                self.duration
            )));
        }
        for (flag, fps) in [("--fps", Some(self.fps)), ("--step-fps", self.step_fps)] {
            if let Some(fps) = fps.filter(|&fps| gif && fps > MAX_GIF_FPS) {
                return Err(usage(format!(
                    "{flag} {fps} is too fast for a GIF, which shows at most {MAX_GIF_FPS} frames a second; an MP4 can have up to {MAX_MP4_FPS}"
                )));
            }
        }
        if let Some(step) = self.step_fps
            && step > self.fps
        {
            return Err(usage(format!(
                "--step-fps {step} is more than --fps {fps}; use a value from 1 to {fps}",
                fps = self.fps
            )));
        }
        let timeline = Timeline {
            duration_s: self.duration,
            fps: self.fps,
            step_fps: self.step_fps,
            intro_s: self.intro,
            hold_s: self.hold,
            loop_back: self.loop_back,
        };
        let draw_s = timeline.draw_s();
        if draw_s < MIN_DRAW_S - 1e-4 {
            return Err(usage(format!(
                "a {}s animation leaves {:.2}s to draw the route after the intro{} and the hold; it needs at least {MIN_DRAW_S}s, so shorten --intro or --hold or lengthen --duration",
                self.duration,
                draw_s.max(0.0),
                if self.loop_back { ", the rewind" } else { "" }
            )));
        }
        Ok(Motion {
            timeline,
            orbit: Orbit {
                sweep_deg: self.orbit,
            },
            pace: match self.pace {
                PaceArg::Distance => Pace::Distance,
                PaceArg::Time => Pace::Time,
            },
            sun: None,
            conditions: Conditions::default(),
            aim: Aim::default(),
            pacing: Pacing::default(),
        })
    }
}

fn seconds(value: &str) -> Result<f32, String> {
    let number = value.strip_suffix('s').unwrap_or(value);
    number
        .trim()
        .parse()
        .ok()
        .filter(|seconds: &f32| seconds.is_finite())
        .ok_or_else(|| "expected seconds, such as 10s or 2.5".to_owned())
}

pub(crate) fn parse_duration(value: &str) -> Result<f32, String> {
    seconds(value)
        .ok()
        .filter(|seconds| (1.0..=MAX_MP4_DURATION_S).contains(seconds))
        .ok_or_else(|| {
            "expected seconds from 1 to 180, such as 10s (a GIF can be at most 60)".to_owned()
        })
}

pub(crate) fn parse_part(value: &str) -> Result<f32, String> {
    seconds(value)
        .ok()
        .filter(|seconds| (0.0..=MAX_MP4_DURATION_S).contains(seconds))
        .ok_or_else(|| "expected seconds from 0 to 180, such as 2s".to_owned())
}

pub(crate) fn parse_orbit(value: &str) -> Result<f32, String> {
    parse_within(value, &(-360.0..=360.0))
}

fn parse_fps(value: &str) -> Result<u32, String> {
    parse_count(value, &(1..=MAX_MP4_FPS))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    #[command(allow_negative_numbers = true)]
    struct Cli {
        #[command(flatten)]
        animation: AnimationArgs,
    }

    fn args(flags: &[&str]) -> AnimationArgs {
        let mut line = vec!["isotrail"];
        line.extend(flags);
        Cli::try_parse_from(line).unwrap().animation
    }

    #[test]
    fn durations_take_an_optional_unit() {
        assert_eq!(parse_duration("10s"), Ok(10.0));
        assert_eq!(parse_duration("2.5"), Ok(2.5));
        assert_eq!(parse_duration("180s"), Ok(180.0));
        for bad in ["0.5s", "181", "10m", "s", "ten", "inf"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_part("0"), Ok(0.0));
        assert!(parse_part("-1").is_err());
    }

    #[test]
    fn orbits_stay_within_a_turn_either_way() {
        assert_eq!(parse_orbit("0"), Ok(0.0));
        assert_eq!(parse_orbit("360"), Ok(360.0));
        assert_eq!(parse_orbit("-30"), Ok(-30.0));
        assert!(parse_orbit("-361").is_err());
        assert!(parse_orbit("361").is_err());
        assert_eq!(args(&["--orbit", "-45"]).orbit, -45.0);
    }

    #[test]
    fn gifs_are_shorter_and_slower_than_mp4s() {
        let long = args(&["--duration", "90"]);
        assert!(long.motion(false).is_ok());
        let message = long.motion(true).unwrap_err().to_string();
        assert!(message.contains("60s"), "{message}");
        let fast = args(&["--fps", "60"]);
        assert_eq!(fast.motion(false).unwrap().timeline.fps, 60);
        let message = fast.motion(true).unwrap_err().to_string();
        assert!(message.contains("--fps 60"), "{message}");
        assert!(
            args(&["--fps", "60", "--step-fps", "55"])
                .motion(true)
                .is_err()
        );
    }

    #[test]
    fn intro_and_hold_must_leave_time_to_draw() {
        let motion = args(&["--intro", "0", "--hold", "3s"])
            .motion(true)
            .unwrap();
        assert_eq!(motion.timeline.intro_s, Some(0.0));
        assert_eq!(motion.timeline.hold_s, Some(3.0));
        let squeezed = args(&["--duration", "4", "--intro", "1", "--hold", "2.6"]);
        let message = squeezed.motion(true).unwrap_err().to_string();
        assert!(message.contains("--intro or --hold"), "{message}");
        let looped = args(&["--duration", "2", "--hold", "1.2", "--loop-back"]);
        assert!(looped.motion(true).is_err());
        assert!(
            args(&["--loop-back"])
                .motion(true)
                .unwrap()
                .timeline
                .loop_back
        );
    }
}
