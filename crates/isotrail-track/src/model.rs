use chrono::{DateTime, Utc};
use tracing::{debug, warn};

use crate::{TrackError, clean};

pub const MIN_SEGMENT_POINTS: usize = 2;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    pub name: Option<String>,
    pub desc: Option<String>,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Segment {
    pub points: Vec<TrackPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint {
    pub lat: f64,
    pub lon: f64,
    pub ele: Option<f64>,
    pub time: Option<DateTime<Utc>>,
}

impl Track {
    /// Ignores glitched times, drops stray points and segments too short to
    /// draw, warning about each, and warns when the times run backwards
    pub fn clean(mut self) -> Result<Track, TrackError> {
        let glitched = clean::clear_glitched_times(&mut self);
        if glitched > 0 {
            warn!(
                "ignored {glitched} timestamp{} before 1990 or weeks away from the rest of the hike",
                plural(glitched)
            );
        }
        let strays = clean::drop_stray_points(&mut self);
        if strays > 0 {
            warn!(
                "dropped {strays} stray point{} far from {}",
                plural(strays),
                if strays == 1 {
                    "its neighbours"
                } else {
                    "their neighbours"
                }
            );
        }
        let backward = clean::backward_steps(&self);
        if backward > 0 {
            warn!(
                "the times run backwards in {backward} place{}; the hike starts and ends at its earliest and latest times",
                plural(backward)
            );
        }

        let before = self.segments.len();
        self.segments
            .retain(|segment| segment.points.len() >= MIN_SEGMENT_POINTS);
        let dropped = before - self.segments.len();
        if dropped > 0 {
            debug!(
                dropped,
                "dropped segments with fewer than {MIN_SEGMENT_POINTS} points"
            );
        }
        if self.segments.is_empty() {
            return Err(TrackError::EmptyTrack);
        }
        Ok(self)
    }

    pub fn points(&self) -> impl Iterator<Item = &TrackPoint> {
        self.segments.iter().flat_map(|segment| &segment.points)
    }

    pub fn point_count(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.points.len())
            .sum()
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(len: usize) -> Segment {
        let point = TrackPoint {
            lat: 46.5,
            lon: 10.0,
            ele: None,
            time: None,
        };
        Segment {
            points: vec![point; len],
        }
    }

    #[test]
    fn clean_drops_short_segments_and_keeps_the_rest_in_order() {
        let track = Track {
            name: Some("Loop".into()),
            desc: None,
            segments: vec![segment(1), segment(3), segment(0), segment(2), segment(1)],
        };
        let clean = track.clean().unwrap();
        let lengths: Vec<_> = clean.segments.iter().map(|s| s.points.len()).collect();
        assert_eq!(lengths, [3, 2]);
        assert_eq!(clean.point_count(), 5);
        assert_eq!(clean.points().count(), 5);
        assert_eq!(clean.name.as_deref(), Some("Loop"));
    }

    #[test]
    fn clean_fails_when_no_segment_is_long_enough() {
        let track = Track {
            segments: vec![segment(1), segment(0)],
            ..Track::default()
        };
        assert!(matches!(track.clean(), Err(TrackError::EmptyTrack)));
        assert!(matches!(
            Track::default().clean(),
            Err(TrackError::EmptyTrack)
        ));
    }
}
