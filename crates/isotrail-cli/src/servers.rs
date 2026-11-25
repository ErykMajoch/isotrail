use std::env;

use isotrail_geo::ApiKey;
pub use isotrail_options::data::{CacheDirArg, WeatherUrlArg};

const KEY_ENV: &str = "ISOTRAIL_OPEN_METEO_KEY";

/// The key for Open-Meteo's paid service, from the environment
pub fn key() -> Option<ApiKey> {
    env::var(KEY_ENV)
        .ok()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .map(ApiKey)
}
