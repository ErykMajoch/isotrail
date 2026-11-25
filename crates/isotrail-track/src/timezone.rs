use std::sync::OnceLock;

use chrono_tz::Tz;
use tzf_rs::DefaultFinder;

static FINDER: OnceLock<DefaultFinder> = OnceLock::new();

/// IANA time zone at a point, from bundled boundary data
pub fn timezone_at(lat: f64, lon: f64) -> Option<Tz> {
    #[cfg(not(target_arch = "wasm32"))]
    let finder = FINDER.get_or_init(DefaultFinder::new);
    #[cfg(target_arch = "wasm32")]
    let finder = FINDER.get()?;
    finder.get_tz_name(lon, lat).parse().ok()
}

/// Reads the zone boundaries from tzf-dist's `lite.tzb`, the data built into
/// the native program, which a browser fetches as a file of its own
#[cfg(target_arch = "wasm32")]
pub fn load_timezones(tzb: &[u8]) -> Result<(), tzf_rs::Error> {
    if FINDER.get().is_none() {
        let _ = FINDER.set(DefaultFinder::from_tzb(tzb)?);
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn timezones_loaded() -> bool {
    FINDER.get().is_some()
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
