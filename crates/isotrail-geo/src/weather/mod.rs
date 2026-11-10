use isotrail_bundle::Weather;

mod error;
mod open_meteo;
mod request;

pub use error::WeatherError;
pub use open_meteo::{
    ARCHIVE_LAG_S, OPEN_METEO_ARCHIVE_URL, OPEN_METEO_FORECAST_URL, OpenMeteoSource,
};
pub use request::{LOCATION_SPACING_M, MAX_LOCATIONS, WeatherLocation, WeatherRequest};

/// Hourly weather for the hours of a hike
pub trait WeatherSource {
    fn weather(&self, request: &WeatherRequest) -> Result<Weather, WeatherError>;
}
