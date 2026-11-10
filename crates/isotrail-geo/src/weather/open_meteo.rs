use chrono::DateTime;
use isotrail_bundle::{Weather, WeatherDataset, WeatherSample};
use reqwest::Url;
use serde::Deserialize;
use tracing::{debug, info, warn};

use super::{WeatherError, WeatherRequest, WeatherSource};
use crate::Fetcher;

pub const OPEN_METEO_ARCHIVE_URL: &str = "https://archive-api.open-meteo.com/v1/archive";
pub const OPEN_METEO_FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
/// Hikes that ended more recently than this go to the forecast API, since
/// the archive takes a few days to catch up
pub const ARCHIVE_LAG_S: i64 = 5 * 86_400;
const HOURLY: &str = "temperature_2m,weather_code,cloud_cover,cloud_cover_low,cloud_cover_mid,\
                      cloud_cover_high,precipitation,rain,snowfall,snow_depth,wind_speed_10m,\
                      wind_direction_10m";

#[derive(Debug, Clone)]
pub struct OpenMeteoSource<F> {
    fetcher: F,
    archive_url: String,
    forecast_url: String,
}

impl<F: Fetcher> OpenMeteoSource<F> {
    pub fn new(fetcher: F) -> OpenMeteoSource<F> {
        OpenMeteoSource {
            fetcher,
            archive_url: OPEN_METEO_ARCHIVE_URL.to_owned(),
            forecast_url: OPEN_METEO_FORECAST_URL.to_owned(),
        }
    }

    /// The GET for a request at an archive or forecast endpoint
    pub fn url(endpoint: &str, request: &WeatherRequest) -> String {
        let join = |values: Vec<f64>| {
            let parts: Vec<String> = values.iter().map(f64::to_string).collect();
            parts.join(",")
        };
        let date = |time: i64| {
            DateTime::from_timestamp(time, 0)
                .map(|t| t.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        };
        let locations = &request.locations;
        let first = request.hours.first().copied().unwrap_or_default();
        let mut params = vec![
            ("latitude", join(locations.iter().map(|l| l.lat).collect())),
            ("longitude", join(locations.iter().map(|l| l.lon).collect())),
        ];
        if let Some(heights) = locations.iter().map(|l| l.ele_m).collect() {
            params.push(("elevation", join(heights)));
        }
        params.extend([
            ("start_date", date(first)),
            ("end_date", date(request.last_hour())),
            ("hourly", HOURLY.to_owned()),
            ("timezone", "GMT".to_owned()),
            ("timeformat", "unixtime".to_owned()),
            ("wind_speed_unit", "ms".to_owned()),
        ]);
        Url::parse_with_params(endpoint, &params).map_or_else(|_| endpoint.to_owned(), String::from)
    }

    fn lookup(
        &self,
        endpoint: &str,
        request: &WeatherRequest,
        dataset: WeatherDataset,
    ) -> Result<Weather, WeatherError> {
        let url = Self::url(endpoint, request);
        info!(
            endpoint,
            locations = request.locations.len(),
            hours = request.hours.len(),
            "fetching the weather from Open-Meteo"
        );
        let bytes = self
            .fetcher
            .get(&url)
            .map_err(|source| WeatherError::Fetch { source })?;
        debug!(bytes = bytes.len(), "Open-Meteo response");
        parse(&bytes)
            .and_then(|locations| resolve(&locations, request, dataset))
            .inspect_err(|_| {
                if let Err(err) = self.fetcher.forget(&url, None) {
                    warn!("could not drop the Open-Meteo response from the cache: {err}");
                }
            })
    }
}

impl<F: Fetcher> WeatherSource for OpenMeteoSource<F> {
    fn weather(&self, request: &WeatherRequest) -> Result<Weather, WeatherError> {
        if request.now - request.last_hour() >= ARCHIVE_LAG_S {
            match self.lookup(&self.archive_url, request, WeatherDataset::Archive) {
                Err(err @ WeatherError::Incomplete { .. }) => {
                    info!("{err} in the archive, so trying the forecast API");
                }
                result => return result,
            }
        }
        self.lookup(&self.forecast_url, request, WeatherDataset::Forecast)
    }
}

#[derive(Deserialize)]
struct Location {
    hourly: Hourly,
}

#[derive(Deserialize)]
struct Hourly {
    time: Vec<i64>,
    temperature_2m: Vec<Option<f32>>,
    weather_code: Vec<Option<f32>>,
    cloud_cover: Vec<Option<f32>>,
    cloud_cover_low: Vec<Option<f32>>,
    cloud_cover_mid: Vec<Option<f32>>,
    cloud_cover_high: Vec<Option<f32>>,
    precipitation: Vec<Option<f32>>,
    rain: Vec<Option<f32>>,
    snowfall: Vec<Option<f32>>,
    snow_depth: Vec<Option<f32>>,
    wind_speed_10m: Vec<Option<f32>>,
    wind_direction_10m: Vec<Option<f32>>,
}

impl Hourly {
    fn sample(&self, time: i64) -> Option<WeatherSample> {
        let i = self.time.iter().position(|&t| t == time)?;
        let at = |values: &[Option<f32>]| values.get(i).copied().flatten();
        let percent = |values: &[Option<f32>]| at(values).map(|v| v.clamp(0.0, 100.0));
        let amount = |values: &[Option<f32>]| at(values).map(|v| v.max(0.0));
        Some(WeatherSample {
            time,
            temperature_c: at(&self.temperature_2m)?,
            weather_code: at(&self.weather_code)?.round().clamp(0.0, 255.0) as u8,
            cloud_cover_pct: percent(&self.cloud_cover)?,
            cloud_low_pct: percent(&self.cloud_cover_low)?,
            cloud_mid_pct: percent(&self.cloud_cover_mid)?,
            cloud_high_pct: percent(&self.cloud_cover_high)?,
            precipitation_mm: amount(&self.precipitation)?,
            rain_mm: amount(&self.rain)?,
            snowfall_cm: amount(&self.snowfall)?,
            snow_depth_m: amount(&self.snow_depth)?,
            wind_speed_m_s: amount(&self.wind_speed_10m)?,
            wind_direction_deg: at(&self.wind_direction_10m)?.rem_euclid(360.0),
        })
    }
}

/// One location answers with an object, several with an array
fn parse(bytes: &[u8]) -> Result<Vec<Location>, WeatherError> {
    let json = |source| WeatherError::Json { source };
    if bytes.trim_ascii_start().starts_with(b"[") {
        serde_json::from_slice(bytes).map_err(json)
    } else {
        serde_json::from_slice(bytes)
            .map(|one| vec![one])
            .map_err(json)
    }
}

fn resolve(
    locations: &[Location],
    request: &WeatherRequest,
    dataset: WeatherDataset,
) -> Result<Weather, WeatherError> {
    let samples: Vec<WeatherSample> = request
        .hours
        .iter()
        .zip(&request.hour_locations)
        .filter_map(|(&hour, &at)| locations.get(at)?.hourly.sample(hour))
        .collect();
    let missing = request.hours.len() - samples.len();
    if missing > 0 {
        return Err(WeatherError::Incomplete {
            missing,
            hours: request.hours.len(),
        });
    }
    Ok(Weather { dataset, samples })
}
