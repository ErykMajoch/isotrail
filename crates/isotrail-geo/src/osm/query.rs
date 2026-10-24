use std::fmt::Write as _;

use crate::Grid;

const TIMEOUT_S: u32 = 180;
const NATURAL_AREAS: &str =
    "water|wood|scrub|heath|grassland|bare_rock|scree|shingle|sand|glacier|wetland|fell";
const WATERWAYS: &str = "river|canal";
const MINOR_WATERWAYS: &str = "stream";
const HIGHWAYS: &str = "motorway|trunk|primary|secondary|tertiary";
const MINOR_HIGHWAYS: &str =
    "path|footway|track|bridleway|steps|cycleway|unclassified|residential|living_street";

/// Coarser cells leave out streams, paths and minor roads: they can't be
/// told apart at that scale, and around towns they are most of the download
pub const MINOR_WAY_MAX_CELL_M: f64 = 100.0;

/// Edges in degrees
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub south: f64,
    pub west: f64,
    pub north: f64,
    pub east: f64,
}

impl BoundingBox {
    /// Encloses the grid's slab, rounded outward to 5 decimals (about 1 m)
    /// so the query text, and with it the cache key, is stable
    pub fn around(grid: &Grid) -> BoundingBox {
        let corners = grid
            .footprint
            .corners()
            .map(|(x, y)| grid.footprint.projection.unproject(x, y));
        let (mut south, mut west) = (f64::INFINITY, f64::INFINITY);
        let (mut north, mut east) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (lat, lon) in corners {
            (south, north) = (south.min(lat), north.max(lat));
            (west, east) = (west.min(lon), east.max(lon));
        }
        let down = |v: f64| (v * 1e5).floor() / 1e5;
        let up = |v: f64| (v * 1e5).ceil() / 1e5;
        BoundingBox {
            south: down(south),
            west: down(west),
            north: up(north),
            east: up(east),
        }
    }
}

/// Every statement carries its own bbox filter instead of the global
/// `[bbox]` setting, which would also clip the geometry of `out geom`. Ways
/// print with `tags` verbosity to leave out their node lists, while
/// relations need full verbosity to keep their members
pub fn overpass_query(bbox: BoundingBox, cell_size_m: f64) -> String {
    let BoundingBox {
        south,
        west,
        north,
        east,
    } = bbox;
    let b = format!("({south:.5},{west:.5},{north:.5},{east:.5})");
    let multipolygon = "relation[\"type\"=\"multipolygon\"]";
    let (waterways, highways) = if cell_size_m <= MINOR_WAY_MAX_CELL_M {
        (
            format!("{WATERWAYS}|{MINOR_WATERWAYS}"),
            format!("{HIGHWAYS}|{MINOR_HIGHWAYS}"),
        )
    } else {
        (WATERWAYS.to_owned(), HIGHWAYS.to_owned())
    };
    let ways = [
        format!("way[\"natural\"~\"^({NATURAL_AREAS})$\"]{b}"),
        format!("way[\"natural\"=\"cliff\"]{b}"),
        format!("way[\"landuse\"]{b}"),
        format!("way[\"waterway\"~\"^({waterways})$\"]{b}"),
        format!("way[\"highway\"~\"^({highways})(_link)?$\"]{b}"),
    ];
    let relations = [
        format!("{multipolygon}[\"natural\"~\"^({NATURAL_AREAS})$\"]{b}"),
        format!("{multipolygon}[\"landuse\"]{b}"),
    ];
    let mut query = format!("[out:json][timeout:{TIMEOUT_S}];\n");
    for (statements, out) in [(&ways[..], "out tags geom;"), (&relations[..], "out geom;")] {
        query.push_str("(\n");
        for statement in statements {
            let _ = writeln!(query, "  {statement};");
        }
        let _ = writeln!(query, ");\n{out}");
    }
    query
}

/// The query as the `data` field of a form POST
pub(super) fn form_body(query: &str) -> Vec<u8> {
    let mut body = String::from("data=");
    for byte in query.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            body.push(char::from(byte));
        } else {
            let _ = write!(body, "%{byte:02X}");
        }
    }
    body.into_bytes()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use isotrail_track::{Segment, Track, TrackPoint};

    use super::*;
    use crate::Footprint;

    fn grid() -> Grid {
        let at = |lat, lon| TrackPoint {
            lat,
            lon,
            ele: None,
            time: None,
        };
        let track = Track {
            segments: vec![Segment {
                points: vec![at(50.0, 10.0), at(50.03, 10.05)],
            }],
            ..Track::default()
        };
        Grid::new(
            &Footprint::from_track(&track).unwrap(),
            NonZeroU32::new(64).unwrap(),
        )
    }

    fn decode(body: &[u8]) -> String {
        let text = std::str::from_utf8(body)
            .unwrap()
            .strip_prefix("data=")
            .unwrap();
        let mut bytes = Vec::new();
        let mut rest = text.as_bytes();
        while let Some((&first, tail)) = rest.split_first() {
            if first == b'%' {
                bytes.push(
                    u8::from_str_radix(std::str::from_utf8(&tail[..2]).unwrap(), 16).unwrap(),
                );
                rest = &tail[2..];
            } else {
                bytes.push(first);
                rest = tail;
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn bbox_encloses_the_slab_and_rounds_outward() {
        let grid = grid();
        let bbox = BoundingBox::around(&grid);
        assert!(bbox.south < bbox.north && bbox.west < bbox.east);
        for (x, y) in grid.footprint.corners() {
            let (lat, lon) = grid.footprint.projection.unproject(x, y);
            assert!((bbox.south..=bbox.north).contains(&lat), "{lat} {bbox:?}");
            assert!((bbox.west..=bbox.east).contains(&lon), "{lon} {bbox:?}");
        }
        for edge in [bbox.south, bbox.west, bbox.north, bbox.east] {
            assert_eq!(format!("{edge:.5}").parse::<f64>().unwrap(), edge);
        }
        assert!(bbox.south < 50.0 && bbox.north > 50.03);
    }

    const BBOX: BoundingBox = BoundingBox {
        south: 50.1,
        west: -4.25,
        north: 50.2,
        east: -4.125,
    };

    #[test]
    fn query_asks_for_geometry_in_south_west_north_east_order() {
        let query = overpass_query(BBOX, 30.0);
        assert!(query.starts_with("[out:json][timeout:180];\n"), "{query}");
        assert!(query.ends_with(");\nout geom;\n"), "{query}");
        let (ways, relations) = query.split_once("out tags geom;").unwrap();
        assert!(!ways.contains("relation") && !relations.contains("way"));
        assert_eq!(
            query
                .matches("(50.10000,-4.25000,50.20000,-4.12500);")
                .count(),
            7
        );
        for needle in [
            "way[\"natural\"~\"^(water|wood|",
            "relation[\"type\"=\"multipolygon\"][\"natural\"~",
            "way[\"natural\"=\"cliff\"]",
            "relation[\"type\"=\"multipolygon\"][\"landuse\"]",
            "way[\"waterway\"~\"^(river|canal|stream)$\"]",
            "way[\"highway\"~\"^(motorway|trunk|primary|secondary|tertiary|path|footway|",
            "|living_street)(_link)?$\"]",
        ] {
            assert!(query.contains(needle), "{needle} missing from\n{query}");
        }
        assert!(!query.contains("service"));
    }

    #[test]
    fn coarse_cells_leave_out_minor_ways_but_keep_every_area() {
        let fine = overpass_query(BBOX, MINOR_WAY_MAX_CELL_M);
        let coarse = overpass_query(BBOX, MINOR_WAY_MAX_CELL_M + 1.0);
        for minor in ["stream", "path", "footway", "track", "residential"] {
            assert!(fine.contains(minor) && !coarse.contains(minor), "{minor}");
        }
        assert!(coarse.contains("way[\"waterway\"~\"^(river|canal)$\"]"));
        assert!(coarse.contains(
            "way[\"highway\"~\"^(motorway|trunk|primary|secondary|tertiary)(_link)?$\"]"
        ));
        let areas = |q: &str| {
            q.lines()
                .filter(|l| l.contains("natural") || l.contains("landuse"))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(areas(&fine), areas(&coarse));
        assert_eq!(areas(&coarse).len(), 5);
    }

    #[test]
    fn form_body_escapes_everything_but_unreserved_characters() {
        let query = overpass_query(BoundingBox::around(&grid()), 30.0);
        let body = form_body(&query);
        assert!(body.iter().all(|b| b.is_ascii_graphic()));
        assert_eq!(decode(&body), query);
        assert_eq!(form_body("a b=\"ł\"~"), b"data=a%20b%3D%22%C5%82%22~");
    }
}
