//! Writes the synthetic bundles the web embed's browser check loads:
//! `web_check_bundles <dir>` makes `hike.itb` and `weather.itb` in it

use std::path::PathBuf;

use isotrail_testkit::{hike_bundle, synthetic_weather};

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir)?;
    // Credits OpenStreetMap, as real bundles do, so the check can follow its link
    let mut weather = hike_bundle();
    weather.meta.attribution[0] = "© OpenStreetMap contributors".into();
    weather.weather = Some(synthetic_weather(1_749_884_400, 12));
    for (name, bundle) in [("hike.itb", hike_bundle()), ("weather.itb", weather)] {
        let bytes = bundle.to_bytes().map_err(std::io::Error::other)?;
        std::fs::write(dir.join(name), bytes)?;
    }
    eprintln!("wrote hike.itb and weather.itb to {}", dir.display());
    Ok(())
}
