use std::env;
use std::path::{Path, PathBuf};

use clap::Args;
use isotrail_geo::{ApiKey, WeatherServer};

const KEY_ENV: &str = "ISOTRAIL_OPEN_METEO_KEY";

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
    /// The server and the API key from the environment
    pub fn server(&self) -> WeatherServer {
        WeatherServer {
            base: self.weather_url.clone().filter(|url| !url.is_empty()),
            key: env::var(KEY_ENV)
                .ok()
                .map(|key| key.trim().to_owned())
                .filter(|key| !key.is_empty())
                .map(ApiKey),
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
    use super::*;

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
