use anyhow::bail;
use clap::{Args, ValueEnum};
use isotrail_export::MAX_GIF_FPS;
use isotrail_render::{DEFAULT_FPS, DEFAULT_ORBIT_DEG, Motion, Orbit, Pace, Timeline};

const MAX_DURATION_S: f32 = 60.0;

#[derive(Debug, Args)]
pub struct AnimationArgs {
    /// Length of an animation, from 1s to 60s: 0.5 s showing the scene, 8 s
    /// drawing the route and 1.5 s on the finished hike for every 10 s
    #[arg(long, value_name = "SECONDS", default_value = "10s", value_parser = parse_duration)]
    duration: f32,

    /// Frames per second of an animation
    #[arg(
        long,
        value_name = "N",
        default_value_t = DEFAULT_FPS,
        value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_GIF_FPS))
    )]
    fps: u32,

    /// Different images per second, fewer than --fps for choppy, game-like
    /// motion
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_GIF_FPS))
    )]
    step_fps: Option<u32>,

    /// How far the camera turns during an animation, in degrees, easing to
    /// the still's view as the route finishes: 0 keeps it still and 360
    /// makes a full turn
    #[arg(long, value_name = "DEG", default_value_t = DEFAULT_ORBIT_DEG, value_parser = parse_orbit)]
    orbit: f32,

    /// How fast the route draws itself: the same distance every frame, or
    /// following the recorded times with stops cut to a minute
    #[arg(long, value_enum, default_value_t = PaceArg::Distance)]
    pace: PaceArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PaceArg {
    Distance,
    Time,
}

impl AnimationArgs {
    pub fn motion(&self) -> anyhow::Result<Motion> {
        if let Some(step) = self.step_fps
            && step > self.fps
        {
            bail!(
                "--step-fps {step} is more than --fps {}: it can only slow the motion down",
                self.fps
            );
        }
        Ok(Motion {
            timeline: Timeline {
                duration_s: self.duration,
                fps: self.fps,
                step_fps: self.step_fps,
            },
            orbit: Orbit {
                sweep_deg: self.orbit,
            },
            pace: match self.pace {
                PaceArg::Distance => Pace::Distance,
                PaceArg::Time => Pace::Time,
            },
        })
    }
}

fn parse_duration(value: &str) -> Result<f32, String> {
    let number = value.strip_suffix('s').unwrap_or(value);
    let seconds: f32 = number
        .trim()
        .parse()
        .map_err(|_| "expected seconds, such as 10s or 2.5".to_owned())?;
    if (1.0..=MAX_DURATION_S).contains(&seconds) {
        Ok(seconds)
    } else {
        Err("must be between 1s and 60s".into())
    }
}

fn parse_orbit(value: &str) -> Result<f32, String> {
    let degrees: f32 = value.parse().map_err(|e| format!("{e}"))?;
    if (0.0..=360.0).contains(&degrees) {
        Ok(degrees)
    } else {
        Err("must be between 0 and 360".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_take_an_optional_unit() {
        assert_eq!(parse_duration("10s"), Ok(10.0));
        assert_eq!(parse_duration("2.5"), Ok(2.5));
        assert_eq!(parse_duration("60s"), Ok(60.0));
        for bad in ["0.5s", "61", "10m", "s", "ten"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn orbits_stay_within_a_turn() {
        assert_eq!(parse_orbit("0"), Ok(0.0));
        assert_eq!(parse_orbit("360"), Ok(360.0));
        assert!(parse_orbit("-5").is_err());
        assert!(parse_orbit("361").is_err());
    }
}
