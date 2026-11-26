use std::num::NonZeroU32;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use isotrail_bundle::SceneBundle;
use isotrail_geo::{
    ApiKey, DEFAULT_DETAIL, FootprintOptions, MIN_PADDING_M, OVERPASS_URL, Orientation,
    PADDING_FRACTION, SceneOptions, SeasonKind, TERRARIUM_URL, Times, WeatherServer, forced_season,
    without_weather,
};
use isotrail_render::parse_count;
use tracing::warn;

use crate::{Typed, usage};

pub(crate) const EXAGGERATION_RANGE: RangeInclusive<f64> = 0.1..=100.0;
pub(crate) const DETAIL_RANGE: RangeInclusive<u32> = 8..=isotrail_bundle::MAX_GRID_SIDE;

pub fn parse_exaggeration(value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse()
        .ok()
        .filter(|exaggeration| EXAGGERATION_RANGE.contains(exaggeration))
        .ok_or_else(|| "expected a number from 0.1 to 100".to_owned())
}

fn parse_detail(value: &str) -> Result<u32, String> {
    parse_count(value, &DETAIL_RANGE)
}

/// How terrain and map data are fetched for a GPX file
#[derive(Debug, Args)]
#[command(next_help_heading = crate::DATA)]
pub struct DataArgs {
    /// Grid cells along the slab's long side, from 8 to 1024
    #[arg(long, value_name = "N", default_value_t = DEFAULT_DETAIL.get(), value_parser = parse_detail)]
    pub detail: u32,

    /// Only use elevation, map and weather data already in the cache
    #[arg(long)]
    pub offline: bool,

    /// Overpass API server for the OpenStreetMap data
    #[arg(long, value_name = "URL", env = "ISOTRAIL_OVERPASS_URL", default_value = OVERPASS_URL)]
    pub overpass_url: String,

    /// Elevation tiles from a mirror, as an address with {z}, {x} and {y}
    #[arg(long, value_name = "URL", env = "ISOTRAIL_TERRAIN_URL",
          default_value = TERRARIUM_URL, value_parser = parse_template)]
    pub terrain_url: String,

    #[command(flatten)]
    pub weather_url: WeatherUrlArg,

    #[command(flatten)]
    pub cache_dir: CacheDirArg,

    /// Build the diorama from the elevation alone, without lakes, woods and paths
    #[arg(long)]
    pub no_map_data: bool,

    /// Ground round the route: percent from 0 to 100, metres from 0 to 10000 [default: 15,500]
    #[arg(long, value_name = "PERCENT[,METRES]", value_parser = parse_padding,
          allow_hyphen_values = true)]
    pub padding: Option<Padding>,

    /// Which way the slab's sides run: along the route, or north-south and east-west
    #[arg(long, value_enum, value_name = "route|north", default_value_t = FootprintArg::Route,
          hide_possible_values = true)]
    pub footprint: FootprintArg,

    /// Leave out the weather, with the snow and ice it brings
    #[arg(long)]
    pub no_weather: bool,

    /// Season to show: the hike's own (auto), or a season you choose
    #[arg(long, value_enum, value_name = "auto|spring|summer|autumn|winter",
          default_value_t = SeasonArg::Auto, hide_possible_values = true)]
    pub season: SeasonArg,

    /// Times to keep in the scene: as recorded, rounded, or none
    #[arg(long, value_enum, value_name = "recorded|coarse|none",
          default_value_t = TimesArg::Recorded, hide_possible_values = true)]
    pub times: TimesArg,

    /// Cut this far off the start of the route, such as 300m or 1.5km
    #[arg(long, value_name = "DIST", value_parser = parse_distance)]
    pub trim_start: Option<f64>,

    /// Cut this far off the end of the route, such as 300m or 1.5km
    #[arg(long, value_name = "DIST", value_parser = parse_distance)]
    pub trim_end: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TimesArg {
    Recorded,
    Coarse,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    fraction: f64,
    min_m: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FootprintArg {
    Route,
    North,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum SeasonArg {
    #[default]
    Auto,
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl SeasonArg {
    /// The season to force, or none for the hike's own
    pub fn kind(self) -> Option<SeasonKind> {
        match self {
            SeasonArg::Auto => None,
            SeasonArg::Spring => Some(SeasonKind::Spring),
            SeasonArg::Summer => Some(SeasonKind::Summer),
            SeasonArg::Autumn => Some(SeasonKind::Autumn),
            SeasonArg::Winter => Some(SeasonKind::Winter),
        }
    }
}

fn parse_padding(value: &str) -> Result<Padding, String> {
    let (percent, metres) = match value.split_once(',') {
        Some((percent, metres)) => (percent, Some(metres)),
        None => (value, None),
    };
    let number = |text: &str, max: f64, what: &str| -> Result<f64, String> {
        text.trim()
            .parse()
            .ok()
            .filter(|number| (0.0..=max).contains(number))
            .ok_or_else(|| format!("expected the {what} as a number from 0 to {max}"))
    };
    Ok(Padding {
        fraction: number(percent, 100.0, "percentage")? / 100.0,
        min_m: match metres {
            Some(metres) => number(metres, 10_000.0, "metres")?,
            None => MIN_PADDING_M,
        },
    })
}

/// Metres, as a number alone or with `m`, or kilometres with `km`
fn parse_distance(value: &str) -> Result<f64, String> {
    let text = value.trim().to_ascii_lowercase();
    let (number, metres) = match text.strip_suffix("km") {
        Some(number) => (number, 1000.0),
        None => (text.strip_suffix('m').unwrap_or(&text), 1.0),
    };
    number
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number >= 0.0)
        .map(|number| number * metres)
        .ok_or_else(|| "expected a distance such as 300, 300m or 1.5km".to_owned())
}

/// A distance as the docs write it: 450m, 2.84km
pub fn distance_text(metres: f64) -> String {
    if metres < 999.5 {
        format!("{metres:.0}m")
    } else {
        let km = format!("{:.2}", metres / 1000.0);
        format!("{}km", km.trim_end_matches('0').trim_end_matches('.'))
    }
}

impl DataArgs {
    /// The padding as `--padding` takes it, such as 15,500
    pub fn padding_text(&self) -> String {
        let padding = self.padding.unwrap_or(Padding {
            fraction: PADDING_FRACTION,
            min_m: MIN_PADDING_M,
        });
        let round = |value: f64| (value * 1e6).round() / 1e6;
        format!(
            "{},{}",
            round(padding.fraction * 100.0),
            round(padding.min_m)
        )
    }

    /// The library's options, with the weather server and `key`
    pub fn options(&self, key: Option<ApiKey>) -> SceneOptions {
        let padding = self.padding.unwrap_or(Padding {
            fraction: PADDING_FRACTION,
            min_m: MIN_PADDING_M,
        });
        SceneOptions {
            detail: NonZeroU32::new(self.detail).unwrap_or(DEFAULT_DETAIL),
            footprint: FootprintOptions {
                padding_fraction: padding.fraction,
                min_padding_m: padding.min_m,
                orientation: match self.footprint {
                    FootprintArg::Route => Orientation::Route,
                    FootprintArg::North => Orientation::North,
                },
            },
            map_data: !self.no_map_data,
            weather: !self.no_weather,
            season: self.season.kind(),
            times: match self.times {
                TimesArg::Recorded => Times::Recorded,
                TimesArg::Coarse => Times::Coarse,
                TimesArg::None => Times::None,
            },
            trim_start_m: self.trim_start.unwrap_or(0.0),
            trim_end_m: self.trim_end.unwrap_or(0.0),
            terrain_url: self.terrain_url.clone(),
            overpass_url: self.overpass_url.clone(),
            weather_server: self.weather_url.server(key),
        }
    }

    /// Applies `--no-weather` and `--season` to a scene file, which already
    /// holds everything else these options shape
    pub fn apply_to(&self, bundle: &mut SceneBundle) {
        if self.no_weather {
            bundle.weather = None;
            bundle.meta.provenance.weather = None;
            without_weather(&mut bundle.season);
        }
        if let Some(kind) = self.season.kind() {
            bundle.season = forced_season(kind, bundle);
        }
    }

    /// The settings that change the scene made from a GPX file, leaving out
    /// --offline, the cache and the servers, which only change where data
    /// comes from
    pub fn shaping(&self) -> String {
        format!(
            "{:?}",
            (
                self.detail,
                self.no_map_data,
                self.padding,
                self.footprint,
                self.no_weather,
                self.season,
                self.times,
                self.trim_start,
                self.trim_end,
            )
        )
    }

    /// Warns about flags typed for fetching data, which a bundle doesn't need
    pub fn warn_unused_with_bundle(&self, typed: &Typed) {
        for (id, flag) in [
            ("detail", "--detail"),
            ("offline", "--offline"),
            ("overpass_url", "--overpass-url"),
            ("terrain_url", "--terrain-url"),
            ("weather_url", "--weather-url"),
            ("cache_dir", "--cache-dir"),
            ("no_map_data", "--no-map-data"),
            ("padding", "--padding"),
            ("footprint", "--footprint"),
        ] {
            if typed.has(id) {
                warn!(
                    "{flag} does nothing with a scene bundle, which already holds its terrain and map data"
                );
            }
        }
    }

    /// A scene bundle keeps the times and route it was made with, so a
    /// privacy option for them is refused rather than dropped
    pub fn refuse_with_bundle(&self) -> anyhow::Result<()> {
        for (trim, flag) in [
            (self.trim_start, "--trim-start"),
            (self.trim_end, "--trim-end"),
        ] {
            if trim.is_some_and(|metres| metres > 0.0) {
                return Err(usage(format!(
                    "{flag} can't cut the route a scene bundle holds; give the GPX file instead"
                )));
            }
        }
        let times = match self.times {
            TimesArg::Recorded => return Ok(()),
            TimesArg::Coarse => "coarse",
            TimesArg::None => "none",
        };
        Err(usage(format!(
            "--times {times} can't change the times a scene bundle holds; give the GPX file instead"
        )))
    }
}

/// Where downloads are cached
#[derive(Debug, Args)]
pub struct CacheDirArg {
    /// Folder for the downloaded data, instead of the usual cache folder
    #[arg(long, value_name = "DIR", env = "ISOTRAIL_CACHE_DIR", global = true)]
    cache_dir: Option<PathBuf>,
}

impl CacheDirArg {
    pub fn get(&self) -> Option<&Path> {
        self.cache_dir
            .as_deref()
            .filter(|dir| !dir.as_os_str().is_empty())
    }
}

/// Where the weather is looked up
#[derive(Debug, Args)]
pub struct WeatherUrlArg {
    /// Another server with Open-Meteo's API, for the weather
    #[arg(long, value_name = "URL", env = "ISOTRAIL_WEATHER_URL", value_parser = parse_server)]
    weather_url: Option<String>,
}

impl WeatherUrlArg {
    /// The server, with `key` for Open-Meteo's paid service
    pub fn server(&self, key: Option<ApiKey>) -> WeatherServer {
        WeatherServer {
            base: self.weather_url.clone().filter(|url| !url.is_empty()),
            key,
        }
    }
}

fn web_address(value: &str) -> Result<&str, String> {
    let value = value.trim();
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .ok_or("expected an address starting with https:// or http://")?;
    if rest.is_empty() || rest.starts_with('/') {
        return Err("the address has no host".into());
    }
    Ok(value)
}

fn parse_server(value: &str) -> Result<String, String> {
    let url = web_address(value)?;
    if url.contains('?') {
        return Err(
            "give the server's address without a query; isotrail adds /v1/archive and /v1/forecast"
                .into(),
        );
    }
    Ok(url.trim_end_matches('/').to_owned())
}

/// A tile address with `{z}`, `{x}` and `{y}` in it
pub fn parse_template(value: &str) -> Result<String, String> {
    let url = web_address(value)?;
    for part in ["{z}", "{x}", "{y}"] {
        if !url.contains(part) {
            return Err(format!(
                "the address must hold {{z}}, {{x}} and {{y}}, for the tile's zoom, column and row; {part} is missing"
            ));
        }
    }
    Ok(url.to_owned())
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::Season;
    use isotrail_testkit::diorama_bundle;

    use super::*;
    use crate::Settings;

    fn data(json: &str) -> DataArgs {
        Settings::from_json(json, "the test")
            .and_then(|settings| settings.options())
            .unwrap()
            .data
    }

    #[test]
    fn a_scene_file_takes_the_season_and_no_weather() {
        let mut bundle = diorama_bundle();
        bundle.season.snow_line_m = Some(400.0);
        data("{}").apply_to(&mut bundle);
        assert_eq!(bundle.season.snow_line_m, Some(400.0));
        data(r#"{"season": "autumn"}"#).apply_to(&mut bundle);
        assert_eq!(
            bundle.season,
            Season {
                leaf_day: Some(290),
                ..Season::default()
            }
        );
        data(r#"{"season": "winter"}"#).apply_to(&mut bundle);
        assert_eq!(bundle.season.leaf_day, Some(15));
        assert!(bundle.season.snow_line_m.is_some() && bundle.season.ice_line_m.is_some());
        data(r#"{"no-weather": true}"#).apply_to(&mut bundle);
        assert_eq!(
            bundle.season,
            Season {
                leaf_day: Some(15),
                ..Season::default()
            }
        );
        assert!(bundle.weather.is_none() && bundle.meta.provenance.weather.is_none());
    }

    #[test]
    fn trims_are_metres_or_kilometres() {
        for (text, metres) in [
            ("300", 300.0),
            ("300m", 300.0),
            (" 1.5km", 1500.0),
            ("2 KM", 2000.0),
            ("0", 0.0),
        ] {
            assert_eq!(parse_distance(text), Ok(metres), "{text}");
        }
        for bad in ["", "-5", "km", "1.5mi", "nan", "inf", "300 metres"] {
            assert!(parse_distance(bad).is_err(), "{bad}");
        }
        assert_eq!(distance_text(450.2), "450m");
        assert_eq!(distance_text(999.7), "1km");
        assert_eq!(distance_text(2836.0), "2.84km");
        assert_eq!(distance_text(1500.0), "1.5km");
    }

    #[test]
    fn padding_is_a_percentage_and_maybe_metres() {
        assert_eq!(
            parse_padding("10"),
            Ok(Padding {
                fraction: 0.1,
                min_m: MIN_PADDING_M
            })
        );
        assert_eq!(
            parse_padding("0, 250"),
            Ok(Padding {
                fraction: 0.0,
                min_m: 250.0
            })
        );
        for bad in ["", "x", "101", "-1", "10,", "10,20000", "10,5,5"] {
            assert!(parse_padding(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn tile_templates_need_every_part() {
        let aws = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png";
        assert_eq!(parse_template(aws).as_deref(), Ok(aws));
        assert!(parse_template("http://localhost:8080/{z}/{x}/{y}.png").is_ok());
        for bad in [
            "https://example.org/{z}/{x}.png",
            "ftp://example.org/{z}/{x}/{y}.png",
            "example.org/{z}/{x}/{y}.png",
            "https:///{z}/{x}/{y}",
        ] {
            assert!(parse_template(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn weather_servers_are_bare_addresses() {
        assert_eq!(
            parse_server("http://weather.local:8080/").as_deref(),
            Ok("http://weather.local:8080")
        );
        assert!(parse_server("https://example.org/v1/archive?apikey=x").is_err());
        assert!(parse_server("weather.local").is_err());
    }
}
