use isotrail_bundle::{Landcover, line_mask};

use crate::Tags;

/// What a line feature does to the cells it crosses
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LineEffect {
    Cover(Landcover),
    Mask(u8),
}

pub(super) fn area_class(tags: &Tags) -> Option<Landcover> {
    use Landcover::*;
    let natural = match tags.get("natural") {
        Some("water") => Some(Water),
        Some("glacier") => Some(Snow),
        Some("bare_rock") => Some(Rock),
        Some("scree" | "shingle" | "sand") => Some(Scree),
        Some("wood") => Some(forest(tags)),
        Some("scrub") => Some(Scrub),
        Some("heath") => Some(Heath),
        Some("wetland") => Some(Wetland),
        // Fell is open ground above the trees: grass, dwarf shrubs and moss
        Some("grassland" | "fell") => Some(Grassland),
        _ => None,
    };
    natural.or_else(|| match tags.get("landuse")? {
        "reservoir" | "basin" => Some(Water),
        "quarry" => Some(Rock),
        "forest" => Some(forest(tags)),
        "meadow" | "grass" | "village_green" | "recreation_ground" | "flowerbed" => Some(Grassland),
        "farmland"
        | "orchard"
        | "vineyard"
        | "allotments"
        | "plant_nursery"
        | "greenhouse_horticulture"
        | "animal_keeping" => Some(Farmland),
        "residential" | "commercial" | "retail" | "industrial" | "construction" | "brownfield"
        | "railway" | "garages" | "military" | "education" | "religious" | "cemetery"
        | "farmyard" | "landfill" | "highway" => Some(BuiltUp),
        _ => None,
    })
}

fn forest(tags: &Tags) -> Landcover {
    match (tags.get("leaf_type"), tags.get("wood")) {
        (Some("needleleaved"), _) | (None, Some("coniferous")) => Landcover::Coniferous,
        (Some("broadleaved"), _) | (None, Some("deciduous")) => Landcover::Broadleaf,
        _ => Landcover::MixedForest,
    }
}

/// Standing water that gets a level surface; rivers and canals run downhill
pub(super) fn is_lake(tags: &Tags) -> bool {
    !matches!(
        tags.get("water"),
        Some("river" | "stream" | "canal" | "ditch" | "drain")
    )
}

pub(super) fn line_effect(tags: &Tags) -> Option<LineEffect> {
    if tags.is("natural", "cliff") {
        return Some(LineEffect::Cover(Landcover::Rock));
    }
    match tags.get("waterway") {
        Some("river" | "canal") => return Some(LineEffect::Cover(Landcover::Water)),
        Some("stream") => return Some(LineEffect::Mask(line_mask::STREAM)),
        Some(_) => return None,
        None => {}
    }
    match tags.get("highway")? {
        "path" | "footway" | "track" | "bridleway" | "steps" | "cycleway" => {
            Some(LineEffect::Mask(line_mask::PATH))
        }
        _ => Some(LineEffect::Mask(line_mask::ROAD)),
    }
}

/// Which class wins where features overlap: the more specific ground cover
/// beats the broad land use around it
pub(super) fn priority(class: Landcover) -> u8 {
    use Landcover::*;
    match class {
        Water => 10,
        Snow => 9,
        Rock => 8,
        Scree => 7,
        Coniferous | Broadleaf | MixedForest => 6,
        Wetland => 5,
        Scrub => 4,
        Heath => 3,
        Grassland => 2,
        Farmland => 1,
        BuiltUp => 0,
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
    fn forests_follow_their_leaf_type() {
        for (pairs, class) in [
            (&[("natural", "wood")][..], Landcover::MixedForest),
            (
                &[("natural", "wood"), ("leaf_type", "needleleaved")],
                Landcover::Coniferous,
            ),
            (
                &[("landuse", "forest"), ("leaf_type", "broadleaved")],
                Landcover::Broadleaf,
            ),
            (
                &[("landuse", "forest"), ("leaf_type", "mixed")],
                Landcover::MixedForest,
            ),
            (
                &[("landuse", "forest"), ("wood", "coniferous")],
                Landcover::Coniferous,
            ),
            (
                &[("natural", "wood"), ("wood", "deciduous")],
                Landcover::Broadleaf,
            ),
            (
                &[
                    ("natural", "wood"),
                    ("leaf_type", "broadleaved"),
                    ("wood", "coniferous"),
                ],
                Landcover::Broadleaf,
            ),
        ] {
            assert_eq!(area_class(&tags(pairs)), Some(class), "{pairs:?}");
        }
    }

    #[test]
    fn areas_map_to_classes() {
        for (pairs, class) in [
            (&[("natural", "water")][..], Some(Landcover::Water)),
            (&[("landuse", "reservoir")], Some(Landcover::Water)),
            (&[("natural", "glacier")], Some(Landcover::Snow)),
            (&[("natural", "scree")], Some(Landcover::Scree)),
            (&[("natural", "fell")], Some(Landcover::Grassland)),
            (&[("natural", "heath")], Some(Landcover::Heath)),
            (&[("natural", "wetland")], Some(Landcover::Wetland)),
            (&[("landuse", "meadow")], Some(Landcover::Grassland)),
            (&[("landuse", "farmland")], Some(Landcover::Farmland)),
            (&[("landuse", "residential")], Some(Landcover::BuiltUp)),
            (&[("landuse", "quarry")], Some(Landcover::Rock)),
            (
                &[("natural", "scrub"), ("landuse", "residential")],
                Some(Landcover::Scrub),
            ),
            (&[("landuse", "unheard_of")], None),
            (&[("natural", "peak")], None),
            (&[("name", "Somewhere")], None),
        ] {
            assert_eq!(area_class(&tags(pairs)), class, "{pairs:?}");
        }
    }

    #[test]
    fn only_standing_water_is_levelled() {
        assert!(is_lake(&tags(&[("natural", "water")])));
        assert!(is_lake(&tags(&[("natural", "water"), ("water", "lake")])));
        assert!(is_lake(&tags(&[("landuse", "reservoir")])));
        assert!(!is_lake(&tags(&[("natural", "water"), ("water", "river")])));
        assert!(!is_lake(&tags(&[("natural", "water"), ("water", "canal")])));
    }

    #[test]
    fn lines_cover_cells_or_set_mask_bits() {
        for (pairs, effect) in [
            (
                &[("waterway", "river")][..],
                Some(LineEffect::Cover(Landcover::Water)),
            ),
            (
                &[("waterway", "canal")],
                Some(LineEffect::Cover(Landcover::Water)),
            ),
            (
                &[("waterway", "stream")],
                Some(LineEffect::Mask(line_mask::STREAM)),
            ),
            (
                &[("natural", "cliff")],
                Some(LineEffect::Cover(Landcover::Rock)),
            ),
            (
                &[("highway", "footway")],
                Some(LineEffect::Mask(line_mask::PATH)),
            ),
            (
                &[("highway", "track")],
                Some(LineEffect::Mask(line_mask::PATH)),
            ),
            (
                &[("highway", "primary")],
                Some(LineEffect::Mask(line_mask::ROAD)),
            ),
            (
                &[("highway", "residential")],
                Some(LineEffect::Mask(line_mask::ROAD)),
            ),
            (&[("waterway", "dam")], None),
            (&[("barrier", "wall")], None),
        ] {
            assert_eq!(line_effect(&tags(pairs)), effect, "{pairs:?}");
        }
    }

    #[test]
    fn priority_runs_from_water_down_to_built_up() {
        use Landcover::*;
        let order = [
            Water,
            Snow,
            Rock,
            Scree,
            MixedForest,
            Wetland,
            Scrub,
            Heath,
            Grassland,
            Farmland,
            BuiltUp,
        ];
        for pair in order.windows(2) {
            assert!(priority(pair[0]) > priority(pair[1]), "{pair:?}");
        }
        assert_eq!(priority(Coniferous), priority(Broadleaf));
        assert_eq!(priority(Broadleaf), priority(MixedForest));
    }
}
