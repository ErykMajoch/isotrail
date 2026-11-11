use std::borrow::Cow;
use std::mem;

use chrono::{DateTime, NaiveDateTime, Utc};
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use tracing::{debug, warn};

use crate::{Segment, Track, TrackError, TrackPoint, TrackReader};

#[derive(Debug, Clone, Copy, Default)]
pub struct GpxReader;

impl TrackReader for GpxReader {
    fn parse(&self, input: &[u8]) -> Result<Track, TrackError> {
        Parser::new(&decode(input)?).run()
    }
}

/// UTF-8, or UTF-16 when the input starts with its byte-order mark
fn decode(input: &[u8]) -> Result<Cow<'_, str>, TrackError> {
    let unit: fn([u8; 2]) -> u16 = match input {
        [0xFF, 0xFE, ..] => u16::from_le_bytes,
        [0xFE, 0xFF, ..] => u16::from_be_bytes,
        _ => {
            return std::str::from_utf8(input).map(Cow::Borrowed).map_err(|e| {
                TrackError::Encoding {
                    line: line_at(input, e.valid_up_to()),
                }
            });
        }
    };
    let body = &input[2..];
    let (pairs, rest) = body.as_chunks::<2>();
    let units = pairs.iter().map(|&pair| unit(pair));
    let mut text = String::with_capacity(pairs.len());
    for c in char::decode_utf16(units) {
        let Ok(c) = c else {
            return Err(utf16_error(&text));
        };
        text.push(c);
    }
    if !rest.is_empty() {
        return Err(utf16_error(&text));
    }
    Ok(Cow::Owned(text))
}

fn utf16_error(decoded: &str) -> TrackError {
    TrackError::Encoding {
        line: 1 + decoded.matches('\n').count(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    Gpx,
    Metadata,
    Trk,
    Trkseg,
    Trkpt,
    Rte,
    Rtept,
    Name,
    Desc,
    Ele,
    Time,
    Other,
}

impl Tag {
    fn classify(parent: Tag, local_name: &str) -> Tag {
        use Tag::*;
        match (parent, local_name) {
            (Gpx, "metadata") => Metadata,
            (Gpx, "trk") => Trk,
            (Gpx, "rte") => Rte,
            (Trk, "trkseg") => Trkseg,
            (Trkseg, "trkpt") => Trkpt,
            (Rte, "rtept") => Rtept,
            (Gpx | Metadata | Trk | Rte, "name") => Name,
            (Gpx | Metadata | Trk | Rte, "desc") => Desc,
            (Trkpt | Rtept, "ele") => Ele,
            (Trkpt | Rtept, "time") => Time,
            _ => Other,
        }
    }

    fn has_text(self) -> bool {
        matches!(self, Tag::Name | Tag::Desc | Tag::Ele | Tag::Time)
    }
}

#[derive(Default)]
struct Labels {
    name: Option<String>,
    desc: Option<String>,
}

struct Coordinate {
    attribute: &'static str,
    limit: f64,
}

const LAT: Coordinate = Coordinate {
    attribute: "lat",
    limit: 90.0,
};

const LON: Coordinate = Coordinate {
    attribute: "lon",
    limit: 180.0,
};

/// Values that could not be read and were left out
#[derive(Default)]
struct Skipped {
    points: usize,
    elevations: usize,
    times: usize,
    first_line: Option<usize>,
}

impl Skipped {
    fn report(&self) {
        let mut parts = Vec::new();
        if self.points > 0 {
            parts.push(format!(
                "skipped {} without a valid position",
                count(self.points, "point")
            ));
        }
        let unreadable: Vec<String> = [(self.elevations, "elevation"), (self.times, "time")]
            .into_iter()
            .filter(|&(n, _)| n > 0)
            .map(|(n, what)| count(n, what))
            .collect();
        if !unreadable.is_empty() {
            parts.push(format!(
                "{} could not be read and {} treated as missing",
                unreadable.join(" and "),
                if self.elevations + self.times == 1 {
                    "is"
                } else {
                    "are"
                }
            ));
        }
        if let Some(line) = self.first_line {
            warn!("{} (first on line {line})", parts.join("; "));
        }
    }
}

fn count(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

struct Parser<'a> {
    src: &'a str,
    xml: Reader<&'a [u8]>,
    stack: Vec<Tag>,
    text: String,
    metadata: Labels,
    track: Labels,
    route: Labels,
    track_segments: Vec<Segment>,
    route_segments: Vec<Segment>,
    points: Vec<TrackPoint>,
    point: Option<TrackPoint>,
    /// `<trk>` elements with at least one point
    tracks: usize,
    /// Segments before the open `<trk>`
    track_start: usize,
    skipped: Skipped,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Parser {
            src,
            xml: Reader::from_str(src),
            stack: Vec::new(),
            text: String::new(),
            metadata: Labels::default(),
            track: Labels::default(),
            route: Labels::default(),
            track_segments: Vec::new(),
            route_segments: Vec::new(),
            points: Vec::new(),
            point: None,
            tracks: 0,
            track_start: 0,
            skipped: Skipped::default(),
        }
    }

    fn run(mut self) -> Result<Track, TrackError> {
        loop {
            let pos = self.xml.buffer_position();
            let event = self.xml.read_event().map_err(|source| TrackError::Xml {
                line: self.line(self.xml.error_position()),
                source,
            })?;
            match event {
                Event::Start(e) => self.open(&e, pos)?,
                Event::Empty(e) => {
                    self.open(&e, pos)?;
                    self.close(pos)?;
                }
                Event::End(_) => self.close(pos)?,
                Event::Text(t) if self.has_open_text() => self.text.push_str(&t.xml10_content()),
                Event::CData(c) if self.has_open_text() => self.text.push_str(&c.xml10_content()),
                Event::GeneralRef(r) if self.has_open_text() => {
                    let char_ref = r.resolve_char_ref().map_err(|source| TrackError::Xml {
                        line: self.line(pos),
                        source,
                    })?;
                    if let Some(c) = char_ref {
                        self.text.push(c);
                    } else if let Some(s) = resolve_predefined_entity(&r) {
                        self.text.push_str(s);
                    } else {
                        self.text.push('&');
                        self.text.push_str(&r);
                        self.text.push(';');
                    }
                }
                Event::Eof if self.stack.is_empty() => return Err(TrackError::EmptyInput),
                Event::Eof => {
                    return Err(TrackError::Truncated {
                        line: self.line(pos),
                    });
                }
                _ => continue,
            }
            if self.stack.is_empty() {
                return Ok(self.finish());
            }
        }
    }

    fn has_open_text(&self) -> bool {
        self.stack.last().is_some_and(|tag| tag.has_text())
    }

    fn open(&mut self, e: &BytesStart, pos: u64) -> Result<(), TrackError> {
        let local_name = e.local_name();
        let name = local_name.as_ref();
        let tag = match self.stack.last() {
            Some(&parent) => Tag::classify(parent, name),
            None if name == "gpx" => Tag::Gpx,
            None => {
                return Err(TrackError::NotGpx {
                    root: name.to_owned(),
                });
            }
        };
        match tag {
            Tag::Trk => self.track_start = self.track_segments.len(),
            Tag::Trkseg | Tag::Rte => self.points.clear(),
            Tag::Trkpt | Tag::Rtept => {
                let lat = self.coordinate(e, &LAT, pos)?;
                let lon = self.coordinate(e, &LON, pos)?;
                self.point = lat.zip(lon).map(|(lat, lon)| TrackPoint {
                    lat,
                    lon,
                    ele: None,
                    time: None,
                });
                if self.point.is_none() {
                    self.skipped.points += 1;
                    self.note_skip(pos);
                }
            }
            _ if tag.has_text() => self.text.clear(),
            _ => {}
        }
        self.stack.push(tag);
        Ok(())
    }

    fn close(&mut self, pos: u64) -> Result<(), TrackError> {
        let Some(tag) = self.stack.pop() else {
            return Ok(());
        };
        let parent = self.stack.last().copied();
        match tag {
            Tag::Name | Tag::Desc => {
                let labels = match parent {
                    Some(Tag::Gpx | Tag::Metadata) => &mut self.metadata,
                    Some(Tag::Trk) => &mut self.track,
                    Some(Tag::Rte) => &mut self.route,
                    _ => return Ok(()),
                };
                let slot = if tag == Tag::Name {
                    &mut labels.name
                } else {
                    &mut labels.desc
                };
                let value = self.text.trim();
                if slot.is_none() && !value.is_empty() {
                    *slot = Some(value.to_owned());
                }
            }
            Tag::Ele => {
                let value = self.text.trim();
                if !value.is_empty() && self.point.is_some() {
                    let ele = value.parse::<f64>().ok().filter(|ele| ele.is_finite());
                    if ele.is_none() {
                        self.skipped.elevations += 1;
                        self.note_skip(pos);
                    }
                    if let Some(point) = &mut self.point {
                        point.ele = ele;
                    }
                }
            }
            Tag::Time => {
                let value = self.text.trim();
                if !value.is_empty() && self.point.is_some() {
                    let time = parse_time(value);
                    if time.is_none() {
                        self.skipped.times += 1;
                        self.note_skip(pos);
                    }
                    if let Some(point) = &mut self.point {
                        point.time = time;
                    }
                }
            }
            Tag::Trk => {
                if self.track_segments[self.track_start..]
                    .iter()
                    .any(|s| !s.points.is_empty())
                {
                    self.tracks += 1;
                }
            }
            Tag::Trkpt | Tag::Rtept => self.points.extend(self.point.take()),
            Tag::Trkseg => self.track_segments.push(Segment {
                points: mem::take(&mut self.points),
            }),
            Tag::Rte => self.route_segments.push(Segment {
                points: mem::take(&mut self.points),
            }),
            Tag::Gpx | Tag::Metadata | Tag::Other => {}
        }
        Ok(())
    }

    /// `None` when the attribute is missing or not a valid coordinate
    fn coordinate(
        &self,
        e: &BytesStart,
        coordinate: &Coordinate,
        pos: u64,
    ) -> Result<Option<f64>, TrackError> {
        let xml_error = |source: quick_xml::Error| TrackError::Xml {
            line: self.line(pos),
            source,
        };
        let Some(attribute) = e
            .try_get_attribute(coordinate.attribute)
            .map_err(|err| xml_error(err.into()))?
        else {
            return Ok(None);
        };
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(xml_error)?;
        Ok(value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && v.abs() <= coordinate.limit))
    }

    fn note_skip(&mut self, pos: u64) {
        if self.skipped.first_line.is_none() {
            self.skipped.first_line = Some(self.line(pos));
        }
    }

    fn line(&self, pos: u64) -> usize {
        line_at(
            self.src.as_bytes(),
            usize::try_from(pos).unwrap_or(usize::MAX),
        )
    }

    fn finish(self) -> Track {
        self.skipped.report();
        let has_points = |segments: &[Segment]| segments.iter().any(|s| !s.points.is_empty());
        let use_routes = !has_points(&self.track_segments) && has_points(&self.route_segments);

        let (labels, segments) = if use_routes {
            warn!(
                "the file has a planned route but no recorded track; time-based features are off"
            );
            let mut segments = self.route_segments;
            for point in segments.iter_mut().flat_map(|s| &mut s.points) {
                point.time = None;
            }
            (self.route, segments)
        } else {
            if has_points(&self.route_segments) {
                debug!("ignoring planned routes in favour of the recorded track");
            }
            if self.tracks > 1 {
                warn!(
                    "the file has {} tracks; they are joined into one hike",
                    self.tracks
                );
            }
            (self.track, self.track_segments)
        };

        let track = Track {
            name: self.metadata.name.or(labels.name),
            desc: self.metadata.desc.or(labels.desc),
            segments,
        };
        debug!(
            segments = track.segments.len(),
            points = track.point_count(),
            "read GPX"
        );
        track
    }
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(time) = DateTime::parse_from_rfc3339(value) {
        return Some(time.with_timezone(&Utc));
    }
    if let Ok(time) = DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f%z") {
        return Some(time.with_timezone(&Utc));
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|time| time.and_utc())
}

fn line_at(input: &[u8], pos: usize) -> usize {
    1 + input[..pos.min(input.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;

    fn parse(gpx: &str) -> Result<Track, TrackError> {
        GpxReader.parse(gpx.as_bytes())
    }

    fn utc(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 7, 1, h, m, s).unwrap()
    }

    #[test]
    fn reads_points_with_elevation_and_time() {
        let track = parse(
            r#"<?xml version="1.0"?>
            <gpx xmlns="http://www.topografix.com/GPX/1/1" version="1.1">
              <trk><trkseg>
                <trkpt lat="46.50000" lon="10.00000"><ele>2010.25</ele><time>2025-07-01T08:00:00Z</time></trkpt>
                <trkpt lat="-46.5" lon="-10.5"><ele>-3</ele><time>2025-07-01T08:00:03.500Z</time></trkpt>
              </trkseg></trk>
            </gpx>"#,
        )
        .unwrap();
        let points = &track.segments[0].points;
        assert_eq!(
            points[0],
            TrackPoint {
                lat: 46.5,
                lon: 10.0,
                ele: Some(2010.25),
                time: Some(utc(8, 0, 0)),
            }
        );
        assert_eq!((points[1].lat, points[1].lon), (-46.5, -10.5));
        assert_eq!(points[1].ele, Some(-3.0));
        assert_eq!(
            points[1].time,
            Some(utc(8, 0, 3) + Duration::milliseconds(500))
        );
    }

    #[test]
    fn accepts_time_offsets_and_missing_zone() {
        assert_eq!(parse_time("2025-07-01T10:00:00+02:00"), Some(utc(8, 0, 0)));
        assert_eq!(parse_time("2025-07-01T08:00:00"), Some(utc(8, 0, 0)));
        assert_eq!(
            parse_time("2025-07-01T08:00:00.250"),
            Some(utc(8, 0, 0) + Duration::milliseconds(250))
        );
        assert_eq!(parse_time("yesterday"), None);
    }

    #[test]
    fn metadata_name_wins_over_track_name() {
        let track = parse(
            r#"<gpx><metadata><name>From metadata</name></metadata>
               <trk><name>From track</name><desc>Track desc</desc></trk></gpx>"#,
        )
        .unwrap();
        assert_eq!(track.name.as_deref(), Some("From metadata"));
        assert_eq!(track.desc.as_deref(), Some("Track desc"));
    }

    #[test]
    fn empty_labels_fall_back_and_are_trimmed() {
        let track = parse(
            r#"<gpx><metadata><name><![CDATA[  ]]></name><desc><![CDATA[]]></desc></metadata>
               <trk><name>
                 Ridge walk
               </name></trk></gpx>"#,
        )
        .unwrap();
        assert_eq!(track.name.as_deref(), Some("Ridge walk"));
        assert_eq!(track.desc, None);
    }

    #[test]
    fn reads_gpx_1_0_root_name() {
        let track = parse(
            r#"<gpx version="1.0"><name>Old style</name><time>2025-07-01T08:00:00Z</time></gpx>"#,
        )
        .unwrap();
        assert_eq!(track.name.as_deref(), Some("Old style"));
    }

    #[test]
    fn ignores_waypoints_extensions_and_point_names() {
        let track = parse(
            r#"<gpx>
              <wpt lat="1" lon="2"><name>Hut</name><ele>5</ele></wpt>
              <trk><name>Real name</name><extensions><name>Not this</name></extensions>
                <trkseg>
                  <trkpt lat="46.5" lon="10"><name>Point</name><extensions><ele>99</ele></extensions></trkpt>
                  <trkpt lat="46.6" lon="10"/>
                </trkseg>
              </trk>
            </gpx>"#,
        )
        .unwrap();
        assert_eq!(track.name.as_deref(), Some("Real name"));
        assert_eq!(track.point_count(), 2);
        assert!(track.points().all(|p| p.ele.is_none()));
    }

    #[test]
    fn resolves_entities_and_character_references() {
        let track = parse(
            r#"<gpx><trk><name>A &amp; B &lt;1&gt; &#322;&#x105;ka &apos;x&quot; &nbsp;</name></trk></gpx>"#,
        )
        .unwrap();
        assert_eq!(track.name.as_deref(), Some("A & B <1> łąka 'x\" &nbsp;"));
    }

    #[test]
    fn concatenates_segments_of_several_tracks() {
        let track = parse(
            r#"<gpx>
              <trk><trkseg><trkpt lat="1" lon="1"/><trkpt lat="1" lon="2"/></trkseg></trk>
              <trk><name>Second</name><trkseg><trkpt lat="2" lon="1"/></trkseg>
                   <trkseg><trkpt lat="3" lon="1"/><trkpt lat="3" lon="2"/></trkseg></trk>
            </gpx>"#,
        )
        .unwrap();
        assert_eq!(track.name.as_deref(), Some("Second"));
        let lengths: Vec<_> = track.segments.iter().map(|s| s.points.len()).collect();
        assert_eq!(lengths, [2, 1, 2]);
    }

    #[test]
    fn prefers_tracks_over_routes() {
        let track = parse(
            r#"<gpx>
              <rte><name>Plan</name><rtept lat="1" lon="1"/><rtept lat="1" lon="2"/><rtept lat="1" lon="3"/></rte>
              <trk><name>Recording</name><trkseg>
                <trkpt lat="2" lon="1"><time>2025-07-01T08:00:00Z</time></trkpt>
                <trkpt lat="2" lon="2"><time>2025-07-01T08:00:03Z</time></trkpt>
              </trkseg></trk>
            </gpx>"#,
        )
        .unwrap();
        assert_eq!(track.name.as_deref(), Some("Recording"));
        assert_eq!(track.point_count(), 2);
        assert!(track.points().all(|p| p.time.is_some()));
    }

    #[test]
    fn falls_back_to_routes_without_times() {
        let track = parse(
            r#"<gpx>
              <trk><name>Empty</name><trkseg/></trk>
              <rte><name>Plan</name>
                <rtept lat="1" lon="1"><ele>10</ele><time>2025-07-01T08:00:00Z</time></rtept>
                <rtept lat="1" lon="2"><ele>11</ele></rtept>
              </rte>
              <rte><rtept lat="2" lon="1"/><rtept lat="2" lon="2"/></rte>
            </gpx>"#,
        )
        .unwrap();
        assert_eq!(track.name.as_deref(), Some("Plan"));
        assert_eq!(track.segments.len(), 2);
        assert_eq!(track.segments[0].points[0].ele, Some(10.0));
        assert!(track.points().all(|p| p.time.is_none()));
    }

    #[test]
    fn empty_values_are_not_recorded() {
        let track = parse(
            r#"<gpx><trk><trkseg><trkpt lat="1" lon="1"><ele> </ele><time/></trkpt></trkseg></trk></gpx>"#,
        )
        .unwrap();
        let point = track.segments[0].points[0];
        assert_eq!((point.ele, point.time), (None, None));
    }

    #[test]
    fn skips_utf8_bom() {
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend_from_slice(b"<gpx><trk><name>BOM</name></trk></gpx>");
        let track = GpxReader.parse(&bytes).unwrap();
        assert_eq!(track.name.as_deref(), Some("BOM"));
    }

    #[test]
    fn ignores_content_after_the_root() {
        let track = parse("<gpx><trk><name>Done</name></trk></gpx>\n<junk>").unwrap();
        assert_eq!(track.name.as_deref(), Some("Done"));
    }

    #[test]
    fn reads_from_a_stream() {
        let mut input: &[u8] = b"<gpx><trk><name>Stream</name></trk></gpx>";
        let track = GpxReader.read(&mut input).unwrap();
        assert_eq!(track.name.as_deref(), Some("Stream"));
    }

    #[test]
    fn rejects_empty_input() {
        assert!(matches!(parse(""), Err(TrackError::EmptyInput)));
        assert!(matches!(parse("  \n "), Err(TrackError::EmptyInput)));
        assert!(matches!(
            parse(r#"<?xml version="1.0"?><!-- nothing -->"#),
            Err(TrackError::EmptyInput)
        ));
    }

    #[test]
    fn rejects_other_root_elements() {
        let err = parse(r#"<kml xmlns="http://www.opengis.net/kml/2.2"></kml>"#).unwrap_err();
        assert!(matches!(&err, TrackError::NotGpx { root } if root == "kml"));
        assert_eq!(err.to_string(), "not a GPX file: the root element is <kml>");
    }

    #[test]
    fn rejects_truncated_input() {
        let err = parse("<gpx>\n<trk>\n<trkseg>\n<trkpt lat=\"1\" lon=\"1\"/>\n").unwrap_err();
        assert!(matches!(err, TrackError::Truncated { line: 5 }), "{err:?}");
    }

    #[test]
    fn rejects_malformed_xml_with_line() {
        let err = parse("<gpx>\n<trk>\n</trkseg>\n</gpx>").unwrap_err();
        assert!(matches!(err, TrackError::Xml { line: 3, .. }), "{err:?}");
    }

    #[test]
    fn rejects_invalid_utf8_with_line() {
        let err = GpxReader
            .parse(b"<gpx>\n<trk><name>\xFF</name></trk></gpx>")
            .unwrap_err();
        assert!(matches!(err, TrackError::Encoding { line: 2 }), "{err:?}");
    }

    #[test]
    fn skips_points_without_a_valid_position() {
        let track = parse(
            r#"<gpx><trk><trkseg>
              <trkpt lat="1" lon="1"/>
              <trkpt lat="1"><ele>5</ele><time>2025-07-01T08:00:00Z</time></trkpt>
              <trkpt lon="1"/>
              <trkpt lat="91" lon="0"/>
              <trkpt lat="0" lon="-180.5"/>
              <trkpt lat="NaN" lon="0"/>
              <trkpt lat="1,5" lon="0"/>
              <trkpt lat="2" lon="2"/>
            </trkseg></trk><rte><rtept lon="1"/></rte></gpx>"#,
        )
        .unwrap();
        let points: Vec<_> = track.points().map(|p| (p.lat, p.lon)).collect();
        assert_eq!(points, [(1.0, 1.0), (2.0, 2.0)]);
    }

    #[test]
    fn unreadable_elevations_and_times_are_missing() {
        let track = parse(
            r#"<gpx><trk><trkseg>
              <trkpt lat="1" lon="0"><ele>high</ele><time>2025-07-01T08:00:00Z</time></trkpt>
              <trkpt lat="2" lon="0"><ele>inf</ele></trkpt>
              <trkpt lat="3" lon="0"><ele>nan</ele><time>noon</time></trkpt>
              <trkpt lat="4" lon="0"><ele>7</ele><time>2025-07-01T08:00:09+0000</time></trkpt>
            </trkseg></trk></gpx>"#,
        )
        .unwrap();
        let points = &track.segments[0].points;
        assert_eq!(points.len(), 4);
        let eles: Vec<_> = points.iter().map(|p| p.ele).collect();
        assert_eq!(eles, [None, None, None, Some(7.0)]);
        let times: Vec<_> = points.iter().map(|p| p.time).collect();
        assert_eq!(times, [Some(utc(8, 0, 0)), None, None, Some(utc(8, 0, 9))]);
    }

    #[test]
    fn accepts_offsets_without_a_colon() {
        assert_eq!(parse_time("2025-07-01T08:00:00+0000"), Some(utc(8, 0, 0)));
        assert_eq!(parse_time("2025-07-01T10:00:00.5+0200"), {
            Some(utc(8, 0, 0) + Duration::milliseconds(500))
        });
    }

    fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
        let mut bytes = if big_endian {
            vec![0xFE, 0xFF]
        } else {
            vec![0xFF, 0xFE]
        };
        for unit in text.encode_utf16() {
            bytes.extend(if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
        bytes
    }

    #[test]
    fn reads_utf16_with_a_byte_order_mark() {
        let gpx = r#"<?xml version="1.0" encoding="UTF-16"?>
            <gpx><trk><name>Grań nad łąką</name><trkseg>
              <trkpt lat="1" lon="2"/>
            </trkseg></trk></gpx>"#;
        for big_endian in [false, true] {
            let track = GpxReader.parse(&utf16(gpx, big_endian)).unwrap();
            assert_eq!(track.name.as_deref(), Some("Grań nad łąką"));
            assert_eq!(track.point_count(), 1);
        }
    }

    #[test]
    fn rejects_broken_utf16_with_line() {
        let mut bytes = utf16("<gpx>\n<trk>", false);
        bytes.extend([0x00, 0xD8, b'<', 0x00]);
        let err = GpxReader.parse(&bytes).unwrap_err();
        assert!(matches!(err, TrackError::Encoding { line: 2 }), "{err:?}");
        let mut odd = utf16("<gpx/>", false);
        odd.push(b'x');
        let err = GpxReader.parse(&odd).unwrap_err();
        assert!(matches!(err, TrackError::Encoding { line: 1 }), "{err:?}");
    }

    #[test]
    fn counts_lines_from_one() {
        assert_eq!(line_at(b"", 0), 1);
        assert_eq!(line_at(b"a\nb\nc", 2), 2);
        assert_eq!(line_at(b"a\nb\nc", 100), 3);
    }
}
