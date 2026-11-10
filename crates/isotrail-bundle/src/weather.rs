use serde::{Deserialize, Serialize};

pub const WEATHER_ATTRIBUTION: &str = "Weather data by Open-Meteo.com";

/// Snowfall in cm per mm of water, as Open-Meteo reckons it
const SNOW_CM_PER_MM: f32 = 0.7;
const TRACE_MM: f32 = 0.05;

/// Hourly weather around the hike, from the hour before the start to the
/// hour after the end
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub dataset: WeatherDataset,
    pub samples: Vec<WeatherSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeatherDataset {
    /// Reanalysis from the historical archive
    Archive,
    /// The forecast models' own past hours, for hikes too recent for the archive
    Forecast,
}

/// One hour as Open-Meteo reports it: values at `time`, except the sums and
/// the weather code, which cover the hour that ends at `time`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeatherSample {
    /// Unix seconds, on the hour
    pub time: i64,
    pub temperature_c: f32,
    /// WMO weather interpretation code
    pub weather_code: u8,
    pub cloud_cover_pct: f32,
    pub cloud_low_pct: f32,
    pub cloud_mid_pct: f32,
    pub cloud_high_pct: f32,
    pub precipitation_mm: f32,
    pub rain_mm: f32,
    pub snowfall_cm: f32,
    pub snow_depth_m: f32,
    pub wind_speed_m_s: f32,
    /// Where the wind comes from, as a compass bearing
    pub wind_direction_deg: f32,
}

/// The weather at one moment
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherState {
    pub temperature_c: f32,
    pub weather_code: u8,
    pub cloud_cover_pct: f32,
    pub cloud_low_pct: f32,
    pub cloud_mid_pct: f32,
    pub cloud_high_pct: f32,
    pub precipitation: Precipitation,
    /// Water falling, in mm per hour
    pub intensity_mm_h: f32,
    pub snowfall_cm_h: f32,
    pub snow_depth_m: f32,
    pub wind_speed_m_s: f32,
    pub wind_direction_deg: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
    Mixed,
}

impl Weather {
    /// Values in between hours are interpolated; the hour's sums and code
    /// hold through the hour they cover. Times outside the samples take the
    /// nearest one
    ///
    /// # Panics
    /// When there are no samples
    pub fn at(&self, time_s: f64) -> WeatherState {
        let samples = &self.samples;
        let ending = samples
            .partition_point(|s| (s.time as f64) < time_s)
            .min(samples.len() - 1);
        let next = samples.partition_point(|s| s.time as f64 <= time_s);
        let (a, b, t) = match next {
            0 => (&samples[0], &samples[0], 0.0),
            n if n == samples.len() => (&samples[n - 1], &samples[n - 1], 0.0),
            n => {
                let (a, b) = (&samples[n - 1], &samples[n]);
                (
                    a,
                    b,
                    ((time_s - a.time as f64) / (b.time - a.time) as f64) as f32,
                )
            }
        };
        let lerp = |f: fn(&WeatherSample) -> f32| f(a) + (f(b) - f(a)) * t;
        let turn = (b.wind_direction_deg - a.wind_direction_deg + 540.0).rem_euclid(360.0) - 180.0;
        let hour = &samples[ending];
        WeatherState {
            temperature_c: lerp(|s| s.temperature_c),
            weather_code: hour.weather_code,
            cloud_cover_pct: lerp(|s| s.cloud_cover_pct),
            cloud_low_pct: lerp(|s| s.cloud_low_pct),
            cloud_mid_pct: lerp(|s| s.cloud_mid_pct),
            cloud_high_pct: lerp(|s| s.cloud_high_pct),
            precipitation: hour.precipitation(),
            intensity_mm_h: hour.precipitation_mm,
            snowfall_cm_h: hour.snowfall_cm,
            snow_depth_m: lerp(|s| s.snow_depth_m),
            wind_speed_m_s: lerp(|s| s.wind_speed_m_s),
            wind_direction_deg: (a.wind_direction_deg + turn * t).rem_euclid(360.0),
        }
    }
}

impl WeatherSample {
    pub fn precipitation(&self) -> Precipitation {
        let snow_mm = self.snowfall_cm / SNOW_CM_PER_MM;
        let liquid_mm = self.rain_mm.max(self.precipitation_mm - snow_mm);
        match (liquid_mm >= TRACE_MM, snow_mm >= TRACE_MM) {
            (true, true) => Precipitation::Mixed,
            (true, false) => Precipitation::Rain,
            (false, true) => Precipitation::Snow,
            (false, false) => Precipitation::None,
        }
    }
}

/// A short lowercase name for a WMO weather code
pub fn weather_condition(code: u8) -> &'static str {
    match code {
        0 => "clear",
        1 => "mainly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 => "fog",
        48 => "rime fog",
        51 => "light drizzle",
        53 => "drizzle",
        55 => "heavy drizzle",
        56 | 57 => "freezing drizzle",
        61 => "light rain",
        63 => "rain",
        65 => "heavy rain",
        66 | 67 => "freezing rain",
        71 => "light snow",
        73 => "snow",
        75 => "heavy snow",
        77 => "snow grains",
        80 => "light showers",
        81 => "showers",
        82 => "heavy showers",
        85 => "snow showers",
        86 => "heavy snow showers",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm with hail",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(hour: i64) -> WeatherSample {
        WeatherSample {
            time: 1_750_000_000 / 3600 * 3600 + hour * 3600,
            temperature_c: 10.0 + hour as f32 * 2.0,
            weather_code: [0, 3, 61][hour as usize % 3],
            cloud_cover_pct: 20.0 * hour as f32,
            cloud_low_pct: 10.0,
            cloud_mid_pct: 0.0,
            cloud_high_pct: 50.0,
            precipitation_mm: [0.0, 0.0, 1.2][hour as usize % 3],
            rain_mm: [0.0, 0.0, 1.2][hour as usize % 3],
            snowfall_cm: 0.0,
            snow_depth_m: 0.0,
            wind_speed_m_s: 4.0,
            wind_direction_deg: [350.0, 10.0, 90.0][hour as usize % 3],
        }
    }

    fn weather() -> Weather {
        Weather {
            dataset: WeatherDataset::Archive,
            samples: (0..3).map(sample).collect(),
        }
    }

    #[test]
    fn values_are_interpolated_between_hours() {
        let w = weather();
        let quarter = w.samples[0].time as f64 + 900.0;
        let state = w.at(quarter);
        assert_eq!(state.temperature_c, 10.5);
        assert_eq!(state.cloud_cover_pct, 5.0);
        assert_eq!(w.at(w.samples[1].time as f64).temperature_c, 12.0);
    }

    #[test]
    fn wind_turns_the_short_way_round_north() {
        let w = weather();
        let half = w.samples[0].time as f64 + 1800.0;
        assert!(w.at(half).wind_direction_deg.abs() < 1e-3);
        let three_quarters = w.samples[0].time as f64 + 2700.0;
        assert!((w.at(three_quarters).wind_direction_deg - 5.0).abs() < 1e-3);
    }

    #[test]
    fn sums_and_code_belong_to_the_hour_they_end() {
        let w = weather();
        let start = w.samples[1].time as f64;
        let within = w.at(start + 1.0);
        assert_eq!(within.weather_code, 61);
        assert_eq!(within.intensity_mm_h, 1.2);
        assert_eq!(within.precipitation, Precipitation::Rain);
        let on_the_hour = w.at(start);
        assert_eq!(on_the_hour.weather_code, 3);
        assert_eq!(on_the_hour.precipitation, Precipitation::None);
    }

    #[test]
    fn times_outside_hold_the_ends() {
        let w = weather();
        let before = w.at(w.samples[0].time as f64 - 7200.0);
        assert_eq!(before.temperature_c, 10.0);
        assert_eq!(before.weather_code, 0);
        let after = w.at(w.samples[2].time as f64 + 7200.0);
        assert_eq!(after.temperature_c, 14.0);
        assert_eq!(after.weather_code, 61);
        assert_eq!(after.wind_direction_deg, 90.0);
    }

    #[test]
    fn precipitation_kind_follows_rain_and_snowfall() {
        let mut s = sample(0);
        assert_eq!(s.precipitation(), Precipitation::None);
        s.snowfall_cm = 0.7;
        s.precipitation_mm = 1.0;
        assert_eq!(s.precipitation(), Precipitation::Snow);
        s.precipitation_mm = 1.5;
        assert_eq!(s.precipitation(), Precipitation::Mixed);
        s.snowfall_cm = 0.0;
        assert_eq!(s.precipitation(), Precipitation::Rain);
        s.precipitation_mm = 0.01;
        assert_eq!(s.precipitation(), Precipitation::None);
    }

    #[test]
    fn codes_have_names() {
        assert_eq!(weather_condition(0), "clear");
        assert_eq!(weather_condition(3), "overcast");
        assert_eq!(weather_condition(75), "heavy snow");
        assert_eq!(weather_condition(99), "thunderstorm with hail");
        assert_eq!(weather_condition(42), "unknown");
    }
}
