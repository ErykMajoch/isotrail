mod error;
mod overpass;
mod query;
mod rings;

use std::collections::{BTreeMap, HashMap};
use std::fmt;

pub use error::OsmError;
pub use overpass::{OVERPASS_URL, OverpassSource};
pub use query::{BoundingBox, MINOR_WAY_MAX_CELL_M, overpass_query};

use crate::Grid;

/// Tag keys that say what a feature is, in the order they are reported
pub const FEATURE_KEYS: [&str; 5] = ["natural", "landuse", "leaf_type", "waterway", "highway"];

/// (lat, lon) points whose first and last point are the same
pub type Ring = Vec<(f64, f64)>;

/// Where map features come from, for the area a grid covers
pub trait FeatureSource {
    fn features(&self, grid: &Grid) -> Result<OsmFeatures, OsmError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OsmKind {
    Way,
    Relation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OsmId {
    pub kind: OsmKind,
    pub id: i64,
}

impl fmt::Display for OsmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            OsmKind::Way => "way",
            OsmKind::Relation => "relation",
        };
        write!(f, "{kind}/{}", self.id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags(BTreeMap<String, String>);

impl Tags {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    pub fn is(&self, key: &str, value: &str) -> bool {
        self.get(key) == Some(value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

impl FromIterator<(String, String)> for Tags {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Tags {
        Tags(iter.into_iter().collect())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    pub outer: Ring,
    pub inner: Vec<Ring>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    pub id: OsmId,
    pub tags: Tags,
    pub polygons: Vec<Polygon>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub id: OsmId,
    pub tags: Tags,
    pub points: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OsmFeatures {
    pub areas: Vec<Area>,
    pub lines: Vec<Line>,
}

impl OsmFeatures {
    /// Parses an Overpass `[out:json]` response made with `out geom`
    pub fn from_overpass_json(bytes: &[u8]) -> Result<OsmFeatures, OsmError> {
        overpass::parse(bytes)
    }

    /// How many features carry each `key=value` of [`FEATURE_KEYS`], most
    /// common first
    pub fn counts(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let tags = self
            .areas
            .iter()
            .map(|a| &a.tags)
            .chain(self.lines.iter().map(|l| &l.tags));
        for tags in tags {
            for key in FEATURE_KEYS {
                if let Some(value) = tags.get(key) {
                    *counts.entry(format!("{key}={value}")).or_default() += 1;
                }
            }
        }
        let mut counts: Vec<_> = counts.into_iter().collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|&(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    #[test]
    fn counts_feature_tags_most_common_first() {
        let id = OsmId {
            kind: OsmKind::Way,
            id: 1,
        };
        let line = |t: &[(&str, &str)]| Line {
            id,
            tags: tags(t),
            points: Vec::new(),
        };
        let features = OsmFeatures {
            areas: vec![Area {
                id,
                tags: tags(&[("natural", "wood"), ("leaf_type", "mixed"), ("name", "x")]),
                polygons: Vec::new(),
            }],
            lines: vec![
                line(&[("highway", "path")]),
                line(&[("highway", "path"), ("surface", "rock")]),
                line(&[("highway", "track")]),
            ],
        };
        assert_eq!(
            features.counts(),
            [
                ("highway=path".to_owned(), 2),
                ("highway=track".to_owned(), 1),
                ("leaf_type=mixed".to_owned(), 1),
                ("natural=wood".to_owned(), 1),
            ]
        );
    }

    #[test]
    fn ids_and_tags_read_naturally() {
        let id = OsmId {
            kind: OsmKind::Relation,
            id: 42,
        };
        assert_eq!(id.to_string(), "relation/42");
        let tags = tags(&[("natural", "water"), ("water", "lake")]);
        assert!(tags.is("natural", "water"));
        assert_eq!(tags.get("water"), Some("lake"));
        assert_eq!(tags.get("landuse"), None);
        assert_eq!(tags.iter().count(), 2);
    }
}
