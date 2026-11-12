use isotrail_bundle::Weather;

mod error;
mod open_meteo;
mod request;

pub use error::WeatherError;
pub use open_meteo::{
    ARCHIVE_LAG_S, ARCHIVE_START, ApiKey, DAYS_BEFORE, DaysBefore, FORECAST_DAYS,
    OPEN_METEO_ARCHIVE_URL, OPEN_METEO_CUSTOMER_ARCHIVE_URL, OPEN_METEO_CUSTOMER_FORECAST_URL,
    OPEN_METEO_FORECAST_URL, OpenMeteoSource, WeatherServer,
};
pub use request::{LOCATION_SPACING_M, MAX_LOCATIONS, WeatherLocation, WeatherRequest};

/// Weather for the hours of a hike and the days before it
pub trait WeatherSource {
    fn weather(&self, request: &WeatherRequest) -> Result<Weather, WeatherError>;

    /// One entry per location of the request
    fn days_before(&self, request: &WeatherRequest) -> Result<Vec<DaysBefore>, WeatherError>;
}
