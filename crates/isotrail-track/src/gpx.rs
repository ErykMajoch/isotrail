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
        let text = std::str::from_utf8(input).map_err(|e| TrackError::Encoding {
            line: line_at(input, e.valid_up_to()),
        })?;
        Parser::new(text).run()
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
    field: &'static str,
    limit: f64,
}

const LAT: Coordinate = Coordinate {
    attribute: "lat",
    field: "latitude",
    limit: 90.0,
};

const LON: Coordinate = Coordinate {
    attribute: "lon",
    field: "longitude",
    limit: 180.0,
};

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
            Tag::Trkseg | Tag::Rte => self.points.clear(),
            Tag::Trkpt | Tag::Rtept => {
                let element = if tag == Tag::Trkpt { "trkpt" } else { "rtept" };
                self.point = Some(TrackPoint {
                    lat: self.coordinate(e, element, &LAT, pos)?,
                    lon: self.coordinate(e, element, &LON, pos)?,
                    ele: None,
                    time: None,
                });
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
                if !value.is_empty() {
                    let ele = value
                        .parse::<f64>()
                        .ok()
                        .filter(|ele| ele.is_finite())
                        .ok_or_else(|| self.invalid(pos, "elevation", value))?;
                    if let Some(point) = &mut self.point {
                        point.ele = Some(ele);
                    }
                }
            }
            Tag::Time => {
                let value = self.text.trim();
                if !value.is_empty() {
                    let time = parse_time(value).ok_or_else(|| self.invalid(pos, "time", value))?;
                    if let Some(point) = &mut self.point {
                        point.time = Some(time);
                    }
                }
            }
            Tag::Trkpt | Tag::Rtept => self.points.extend(self.point.take()),
            Tag::Trkseg => self.track_segments.push(Segment {
                points: mem::take(&mut self.points),
            }),
            Tag::Rte => self.route_segments.push(Segment {
                points: mem::take(&mut self.points),
            }),
            Tag::Gpx | Tag::Metadata | Tag::Trk | Tag::Other => {}
        }
        Ok(())
    }

    fn coordinate(
        &self,
        e: &BytesStart,
        element: &'static str,
        coordinate: &Coordinate,
        pos: u64,
    ) -> Result<f64, TrackError> {
        let xml_error = |source: quick_xml::Error| TrackError::Xml {
            line: self.line(pos),
            source,
        };
        let attribute = e
            .try_get_attribute(coordinate.attribute)
            .map_err(|err| xml_error(err.into()))?
            .ok_or_else(|| TrackError::MissingAttribute {
                line: self.line(pos),
                element,
                attribute: coordinate.attribute,
            })?;
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(xml_error)?;
        value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && v.abs() <= coordinate.limit)
            .ok_or_else(|| self.invalid(pos, coordinate.field, &value))
    }

    fn invalid(&self, pos: u64, field: &'static str, value: &str) -> TrackError {
        TrackError::InvalidValue {
            line: self.line(pos),
            field,
            value: value.to_owned(),
        }
    }

    fn line(&self, pos: u64) -> usize {
        line_at(
            self.src.as_bytes(),
            usize::try_from(pos).unwrap_or(usize::MAX),
        )
    }

    fn finish(self) -> Track {
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
    fn rejects_missing_coordinates() {
        let err = parse("<gpx><trk><trkseg>\n<trkpt lat=\"1\"/></trkseg></trk></gpx>").unwrap_err();
        assert!(
            matches!(
                err,
                TrackError::MissingAttribute {
                    line: 2,
                    element: "trkpt",
                    attribute: "lon"
                }
            ),
            "{err:?}"
        );
        let err = parse(r#"<gpx><rte><rtept lon="1"/></rte></gpx>"#).unwrap_err();
        assert!(
            matches!(
                err,
                TrackError::MissingAttribute {
                    element: "rtept",
                    attribute: "lat",
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn rejects_invalid_values() {
        let cases = [
            (r#"<trkpt lat="91" lon="0"/>"#, "latitude", "91"),
            (r#"<trkpt lat="0" lon="-180.5"/>"#, "longitude", "-180.5"),
            (r#"<trkpt lat="NaN" lon="0"/>"#, "latitude", "NaN"),
            (r#"<trkpt lat="1,5" lon="0"/>"#, "latitude", "1,5"),
            (
                r#"<trkpt lat="1" lon="0"><ele>high</ele></trkpt>"#,
                "elevation",
                "high",
            ),
            (
                r#"<trkpt lat="1" lon="0"><ele>inf</ele></trkpt>"#,
                "elevation",
                "inf",
            ),
            (
                r#"<trkpt lat="1" lon="0"><time>noon</time></trkpt>"#,
                "time",
                "noon",
            ),
        ];
        for (point, expected_field, expected_value) in cases {
            let gpx = format!("<gpx><trk><trkseg>\n{point}</trkseg></trk></gpx>");
            match parse(&gpx) {
                Err(TrackError::InvalidValue { line, field, value }) => {
                    assert_eq!(
                        (line, field, value.as_str()),
                        (2, expected_field, expected_value)
                    );
                }
                other => panic!("{point}: expected an invalid value error, got {other:?}"),
            }
        }
    }

    #[test]
    fn counts_lines_from_one() {
        assert_eq!(line_at(b"", 0), 1);
        assert_eq!(line_at(b"a\nb\nc", 2), 2);
        assert_eq!(line_at(b"a\nb\nc", 100), 3);
    }
}
