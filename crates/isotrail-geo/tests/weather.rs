use std::cell::RefCell;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use isotrail_bundle::{Precipitation, WeatherDataset};
use isotrail_geo::{
    ARCHIVE_LAG_S, FetchError, Fetcher, OPEN_METEO_ARCHIVE_URL, OPEN_METEO_FORECAST_URL,
    OpenMeteoSource, RecordedFetcher, WeatherError, WeatherLocation, WeatherRequest, WeatherSource,
};
use isotrail_track::{GpxReader, TrackReader};

/// 2026-10-01, long after the recorded hike
const LATER: i64 = 1_790_812_800;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn snowdon(now: i64) -> WeatherRequest {
    let mut file = File::open(fixtures().join("gpx/snowdon.gpx")).unwrap();
    let track = GpxReader.read(&mut file).unwrap().clean().unwrap();
    WeatherRequest::from_track(&track, now).unwrap()
}

/// Serves `RecordedFetcher` files and remembers what was asked and dropped
struct Logged {
    inner: RecordedFetcher,
    asked: RefCell<Vec<String>>,
    forgotten: RefCell<Vec<String>>,
}

impl Logged {
    fn new(dir: &Path) -> Logged {
        Logged {
            inner: RecordedFetcher::new(dir),
            asked: RefCell::default(),
            forgotten: RefCell::default(),
        }
    }

    fn record(&self, url: &str, body: &str) {
        let path = self.inner.path_for(url, None).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
}

impl Fetcher for Logged {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.asked.borrow_mut().push(url.to_owned());
        self.inner.get(url)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        self.inner.post(url, body)
    }

    fn forget(&self, url: &str, _body: Option<&[u8]>) -> Result<(), FetchError> {
        self.forgotten.borrow_mut().push(url.to_owned());
        Ok(())
    }
}

/// An hourly response for `hours`, with `null` where `gap` says so
fn response(hours: &[i64], temperature: f32, gap: impl Fn(usize) -> bool) -> String {
    let column = |value: &str| {
        let values: Vec<String> = (0..hours.len())
            .map(|i| {
                if gap(i) {
                    "null".into()
                } else {
                    value.to_owned()
                }
            })
            .collect();
        format!("[{}]", values.join(","))
    };
    let times: Vec<String> = hours.iter().map(i64::to_string).collect();
    format!(
        r#"{{"latitude":53.07,"longitude":-4.07,"hourly":{{"time":[{}],"temperature_2m":{},"weather_code":{},"cloud_cover":{},"cloud_cover_low":{},"cloud_cover_mid":{},"cloud_cover_high":{},"precipitation":{},"rain":{},"snowfall":{},"snow_depth":{},"wind_speed_10m":{},"wind_direction_10m":{}}}}}"#,
        times.join(","),
        column(&temperature.to_string()),
        column("73"),
        column("100"),
        column("100"),
        column("60"),
        column("0"),
        column("0.7"),
        column("0"),
        column("0.49"),
        column("0.2"),
        column("7.5"),
        column("200"),
    )
}

#[test]
fn recorded_archive_covers_every_hour_of_the_hike() {
    let request = snowdon(LATER);
    assert_eq!(request.locations.len(), 1);
    let source = OpenMeteoSource::new(RecordedFetcher::new(fixtures().join("weather")));
    let weather = source.weather(&request).unwrap();
    assert_eq!(weather.dataset, WeatherDataset::Archive);
    let times: Vec<i64> = weather.samples.iter().map(|s| s.time).collect();
    assert_eq!(times, request.hours);
    for s in &weather.samples {
        assert!((-10.0..30.0).contains(&s.temperature_c), "{s:?}");
        assert!((0.0..=100.0).contains(&s.cloud_cover_pct), "{s:?}");
        assert!((0.0..40.0).contains(&s.wind_speed_m_s), "{s:?}");
        assert!((0.0..=360.0).contains(&s.wind_direction_deg), "{s:?}");
    }
    let state = weather.at(request.hours[1] as f64 + 600.0);
    assert!(state.temperature_c.is_finite());
}

#[test]
fn the_request_asks_for_the_hike_s_days_in_utc() {
    let url = OpenMeteoSource::<RecordedFetcher>::url(OPEN_METEO_ARCHIVE_URL, &snowdon(LATER));
    assert!(url.starts_with(OPEN_METEO_ARCHIVE_URL), "{url}");
    for part in [
        "latitude=53.07&",
        "start_date=2025-06-14",
        "end_date=2025-06-14",
        "timezone=GMT",
        "timeformat=unixtime",
        "wind_speed_unit=ms",
        "snow_depth",
        "wind_direction_10m",
    ] {
        assert!(url.contains(part), "{part} in {url}");
    }
}

#[test]
fn recent_hikes_go_to_the_forecast_api() {
    let dir = tempfile::tempdir().unwrap();
    let fetcher = Logged::new(dir.path());
    let request = snowdon(snowdon(0).last_hour() + ARCHIVE_LAG_S - 1);
    let url = OpenMeteoSource::<Logged>::url(OPEN_METEO_FORECAST_URL, &request);
    fetcher.record(&url, &response(&request.hours, 4.5, |_| false));

    let weather = OpenMeteoSource::new(&fetcher).weather(&request).unwrap();
    assert_eq!(weather.dataset, WeatherDataset::Forecast);
    assert_eq!(*fetcher.asked.borrow(), vec![url]);
    let state = weather.at(request.hours[2] as f64);
    assert_eq!(state.temperature_c, 4.5);
    assert_eq!(state.precipitation, Precipitation::Snow);
}

#[test]
fn gaps_in_the_archive_fall_back_to_the_forecast_and_are_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let fetcher = Logged::new(dir.path());
    let request = snowdon(snowdon(0).last_hour() + ARCHIVE_LAG_S);
    let archive = OpenMeteoSource::<Logged>::url(OPEN_METEO_ARCHIVE_URL, &request);
    let forecast = OpenMeteoSource::<Logged>::url(OPEN_METEO_FORECAST_URL, &request);
    let last = request.hours.len() - 1;
    fetcher.record(&archive, &response(&request.hours, 9.0, |i| i == last));
    fetcher.record(&forecast, &response(&request.hours, 8.0, |_| false));

    let weather = OpenMeteoSource::new(&fetcher).weather(&request).unwrap();
    assert_eq!(weather.dataset, WeatherDataset::Forecast);
    assert_eq!(weather.samples[0].temperature_c, 8.0);
    assert_eq!(*fetcher.asked.borrow(), vec![archive.clone(), forecast]);
    assert_eq!(*fetcher.forgotten.borrow(), vec![archive]);
}

#[test]
fn gaps_everywhere_say_how_many_hours_are_missing() {
    let dir = tempfile::tempdir().unwrap();
    let fetcher = Logged::new(dir.path());
    let request = snowdon(0);
    let forecast = OpenMeteoSource::<Logged>::url(OPEN_METEO_FORECAST_URL, &request);
    fetcher.record(&forecast, &response(&request.hours, 8.0, |i| i < 2));

    let err = OpenMeteoSource::new(&fetcher)
        .weather(&request)
        .unwrap_err();
    assert!(
        matches!(err, WeatherError::Incomplete { missing: 2, hours } if hours == request.hours.len()),
        "{err}"
    );
}

#[test]
fn several_locations_come_back_as_an_array() {
    let dir = tempfile::tempdir().unwrap();
    let fetcher = Logged::new(dir.path());
    let mut request = snowdon(0);
    request.locations.push(WeatherLocation {
        lat: 53.1,
        lon: -4.0,
        ele_m: Some(600.0),
    });
    let last = request.hour_locations.len() - 1;
    request.hour_locations[last] = 1;
    let forecast = OpenMeteoSource::<Logged>::url(OPEN_METEO_FORECAST_URL, &request);
    assert!(forecast.contains("latitude=53.07%2C53.1&"), "{forecast}");
    assert!(forecast.contains("elevation=830%2C600&"), "{forecast}");
    let body = format!(
        "[{},{}]",
        response(&request.hours, 1.0, |_| false),
        response(&request.hours, 2.0, |_| false)
    );
    fetcher.record(&forecast, &body);

    let weather = OpenMeteoSource::new(&fetcher).weather(&request).unwrap();
    let temperatures: Vec<f32> = weather.samples.iter().map(|s| s.temperature_c).collect();
    assert_eq!(temperatures[0], 1.0);
    assert_eq!(temperatures[last], 2.0);
}

#[test]
fn unreadable_responses_are_dropped_from_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let fetcher = Logged::new(dir.path());
    let request = snowdon(0);
    let forecast = OpenMeteoSource::<Logged>::url(OPEN_METEO_FORECAST_URL, &request);
    fetcher.record(&forecast, "<html>busy</html>");

    let err = OpenMeteoSource::new(&fetcher)
        .weather(&request)
        .unwrap_err();
    assert!(matches!(err, WeatherError::Json { .. }), "{err}");
    assert_eq!(*fetcher.forgotten.borrow(), vec![forecast]);
}

#[test]
fn missing_recordings_name_the_request() {
    let dir = tempfile::tempdir().unwrap();
    let source = OpenMeteoSource::new(RecordedFetcher::new(dir.path()));
    let err = source.weather(&snowdon(0)).unwrap_err();
    assert!(matches!(err, WeatherError::Fetch { .. }), "{err}");
}
