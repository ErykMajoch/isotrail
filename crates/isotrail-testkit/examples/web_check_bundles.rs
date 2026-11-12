//! Writes the synthetic bundles the web embed's browser check loads:
//! `web_check_bundles <dir>` makes `hike.itb`, `weather.itb`, `timed.itb`
//! and `paced.itb` in it

use std::path::PathBuf;

use isotrail_testkit::{hike_bundle, synthetic_weather};

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir)?;
    // Credits OpenStreetMap, as real bundles do, so the check can follow its link
    let mut weather = hike_bundle();
    weather.meta.attribution[0] = "© OpenStreetMap contributors".into();
    weather.weather = Some(synthetic_weather(1_749_884_400, 12));
    let mut timed = hike_bundle();
    timed.meta.timezone = Some("Europe/Rome".into());
    // Times along the route, the first half walked three times as slowly
    let mut paced = timed.clone();
    let half = paced.route.points.last().map_or(0.0, |p| p.distance_m) / 2.0;
    for point in &mut paced.route.points {
        let slow = point.distance_m.min(half);
        let fast = (point.distance_m - half).max(0.0);
        point.time = Some(1_749_884_400 + (3.0 * slow + fast) as i64);
    }
    for (name, bundle) in [
        ("hike.itb", hike_bundle()),
        ("weather.itb", weather),
        ("timed.itb", timed),
        ("paced.itb", paced),
    ] {
        let bytes = bundle.to_bytes().map_err(std::io::Error::other)?;
        std::fs::write(dir.join(name), bytes)?;
    }
    eprintln!(
        "wrote hike.itb, weather.itb, timed.itb and paced.itb to {}",
        dir.display()
    );
    Ok(())
}
