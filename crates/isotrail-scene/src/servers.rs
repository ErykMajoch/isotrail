use isotrail_geo::{BoundingBox, OVERPASS_URL};

/// VK Maps' Overpass server, which holds the whole planet
pub const VK_MAPS_URL: &str = "https://maps.mail.ru/osm/tools/overpass/api/interpreter";

const DRESEL_HOST: &str = "https://ovp.dresel.systems/";

/// The names a page leaves a built-in server out by, in the order they are
/// asked
pub const SERVER_NAMES: [&str; 3] = ["dresel", "vk-maps", "overpass-api"];

/// The name of a built-in Overpass server, none for any other
pub fn server_name(url: &str) -> Option<&'static str> {
    if url.starts_with(DRESEL_HOST) {
        Some(SERVER_NAMES[0])
    } else if url == VK_MAPS_URL {
        Some(SERVER_NAMES[1])
    } else if url == OVERPASS_URL {
        Some(SERVER_NAMES[2])
    } else {
        None
    }
}

/// dresel.systems' Overpass servers, one per continent, with the regions each
/// holds in full as south, west, north and east edges. A continent's data ends
/// at borders that cross islands and coasts, and a server asked about land
/// outside it answers as if there were nothing there, so the regions keep
/// clear of every border
const DRESEL_REGIONS: &[(&str, &[[f64; 4]])] = &[
    (
        "https://ovp.dresel.systems/api/interpreter",
        &[
            [37.5, -11.0, 72.0, 40.0],
            [36.0, -9.6, 37.5, -1.0],
            [35.8, 12.0, 37.5, 28.2],
            [36.0, 28.2, 37.5, 35.8],
            [37.5, 40.0, 42.2, 43.0],
            [34.8, 23.4, 35.8, 26.4],
            [34.5, 32.2, 35.75, 34.65],
            [63.2, -24.6, 66.6, -13.4],
            [76.3, 10.0, 80.9, 33.6],
        ],
    ),
    (
        "https://ovp.dresel.systems/api-na/interpreter",
        &[
            [24.5, -168.5, 72.0, -80.0],
            [34.0, -80.0, 72.0, -52.0],
            [28.0, -80.0, 34.0, -75.0],
            [18.8, -160.6, 22.4, -154.7],
        ],
    ),
    (
        "https://ovp.dresel.systems/api-ca/interpreter",
        &[
            [7.3, -86.0, 12.0, -78.0],
            [19.8, -85.0, 23.3, -74.1],
            [13.7, -91.7, 15.6, -88.3],
        ],
    ),
    (
        "https://ovp.dresel.systems/api-sa/interpreter",
        &[[-56.0, -82.0, 5.0, -34.0]],
    ),
    (
        "https://ovp.dresel.systems/api-as/interpreter",
        &[[24.0, 123.0, 45.6, 146.0], [10.0, 60.0, 50.0, 123.0]],
    ),
    (
        "https://ovp.dresel.systems/api-af/interpreter",
        &[
            [-35.0, -18.0, 27.5, 32.0],
            [27.5, -13.2, 35.0, 32.0],
            [-11.5, 32.0, 12.0, 51.5],
            [-27.0, 32.0, -13.5, 51.5],
        ],
    ),
    (
        "https://ovp.dresel.systems/api-oc/interpreter",
        &[[-44.0, 112.0, -11.5, 154.0], [-47.5, 166.0, -34.0, 178.8]],
    ),
];

/// The Overpass servers a page asks for the map data inside `boxes`, in
/// order: a continent's server when one holds every box, then two that hold
/// the planet
pub fn map_data_servers(boxes: &[BoundingBox]) -> Vec<String> {
    let continent = DRESEL_REGIONS
        .iter()
        .find(|(_, regions)| boxes.iter().all(|bbox| covered(regions, bbox)));
    continent
        .map(|(url, _)| *url)
        .into_iter()
        .chain([VK_MAPS_URL, OVERPASS_URL])
        .map(str::to_owned)
        .collect()
}

/// Whether the regions together hold the whole box. Cut along every region
/// edge that crosses it, the box falls into cells that are each wholly inside
/// a region or wholly outside them all
fn covered(regions: &[[f64; 4]], bbox: &BoundingBox) -> bool {
    let cuts = |low: f64, high: f64, edges: Vec<f64>| {
        let mut cuts: Vec<f64> = edges
            .into_iter()
            .filter(|&edge| edge > low && edge < high)
            .chain([low, high])
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        cuts
    };
    let lats = cuts(
        bbox.south,
        bbox.north,
        regions.iter().flat_map(|r| [r[0], r[2]]).collect(),
    );
    let lons = cuts(
        bbox.west,
        bbox.east,
        regions.iter().flat_map(|r| [r[1], r[3]]).collect(),
    );
    lats.windows(2).all(|lat| {
        lons.windows(2).all(|lon| {
            let (lat, lon) = (f64::midpoint(lat[0], lat[1]), f64::midpoint(lon[0], lon[1]));
            regions
                .iter()
                .any(|r| (r[0]..=r[2]).contains(&lat) && (r[1]..=r[3]).contains(&lon))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn around(lat: f64, lon: f64, half: f64) -> Vec<BoundingBox> {
        vec![BoundingBox {
            south: lat - half,
            west: lon - half,
            north: lat + half,
            east: lon + half,
        }]
    }

    fn first(lat: f64, lon: f64) -> String {
        map_data_servers(&around(lat, lon, 0.05)).remove(0)
    }

    #[test]
    fn a_slab_inside_a_continent_asks_its_server_first() {
        assert_eq!(
            map_data_servers(&around(53.07, -4.08, 0.05)),
            [
                "https://ovp.dresel.systems/api/interpreter",
                VK_MAPS_URL,
                OVERPASS_URL
            ]
        );
        for (lat, lon, path) in [
            (45.92, 6.87, "api"),
            (47.07, 12.69, "api"),
            (55.75, 37.62, "api"),
            (39.74, -104.99, "api-na"),
            (9.93, -84.08, "api-ca"),
            (-13.53, -71.97, "api-sa"),
            (27.72, 85.32, "api-as"),
            (35.68, 139.69, "api-as"),
            (-3.07, 37.35, "api-af"),
            (-45.03, 168.66, "api-oc"),
        ] {
            assert_eq!(
                first(lat, lon),
                format!("https://ovp.dresel.systems/{path}/interpreter"),
                "{lat}, {lon}"
            );
        }
    }

    #[test]
    fn every_built_in_server_has_its_name() {
        let names: Vec<Option<&str>> = map_data_servers(&around(53.07, -4.08, 0.05))
            .iter()
            .map(|url| server_name(url))
            .collect();
        assert_eq!(names, SERVER_NAMES.map(Some));
        assert_eq!(server_name("https://overpass.example.org/api"), None);
    }

    #[test]
    fn land_near_a_continent_s_border_goes_to_the_planet() {
        for (lat, lon) in [
            (28.47, -16.25),
            (35.77, -5.8),
            (37.27, 9.87),
            (64.18, -51.72),
        ] {
            assert_eq!(first(lat, lon), VK_MAPS_URL, "{lat}, {lon}");
        }
        assert_eq!(map_data_servers(&around(0.0, -150.0, 0.05)).len(), 2);
    }

    #[test]
    fn a_slab_across_two_regions_of_one_continent_stays_with_it() {
        let sicily = vec![BoundingBox {
            south: 37.3,
            west: 14.0,
            north: 37.7,
            east: 14.4,
        }];
        assert_eq!(map_data_servers(&sicily).len(), 3);
        let iceland_to_sea = vec![BoundingBox {
            south: 66.5,
            west: -16.0,
            north: 66.7,
            east: -15.6,
        }];
        assert_eq!(map_data_servers(&iceland_to_sea).len(), 2);
        let two = vec![
            BoundingBox {
                south: 37.6,
                west: 14.0,
                north: 37.7,
                east: 14.4,
            },
            BoundingBox {
                south: 37.3,
                west: 14.0,
                north: 37.4,
                east: 14.4,
            },
        ];
        assert_eq!(map_data_servers(&two).len(), 3);
    }
}
