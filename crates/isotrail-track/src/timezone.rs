use std::sync::OnceLock;

use chrono_tz::Tz;
use tzf_rs::DefaultFinder;

/// IANA time zone at a point, from bundled boundary data
pub fn timezone_at(lat: f64, lon: f64) -> Option<Tz> {
    static FINDER: OnceLock<DefaultFinder> = OnceLock::new();
    let name = FINDER.get_or_init(DefaultFinder::new).get_tz_name(lon, lat);
    name.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_cities() {
        assert_eq!(timezone_at(40.7128, -74.0060), Some(Tz::America__New_York));
        assert_eq!(timezone_at(35.6762, 139.6503), Some(Tz::Asia__Tokyo));
        assert_eq!(timezone_at(-33.8688, 151.2093), Some(Tz::Australia__Sydney));
    }

    #[test]
    fn open_ocean_has_a_nautical_zone() {
        let tz = timezone_at(0.0, -140.0).expect("ocean zone");
        assert!(tz.name().starts_with("Etc/"), "{}", tz.name());
    }
}
