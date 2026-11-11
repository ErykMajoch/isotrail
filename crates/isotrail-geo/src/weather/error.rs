/// Why the weather could not be found
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WeatherError {
    /// The request failed; the message names the host but not the long URL
    #[error("{message}")]
    Fetch { message: String },

    /// Offline mode is on and the answer isn't cached
    #[error("the weather from {host} is not in the cache and offline mode is on")]
    Offline { host: String },

    #[error("could not read the Open-Meteo response")]
    Json {
        #[source]
        source: serde_json::Error,
    },

    /// Some hours are missing or have gaps, as for hikes still in progress
    #[error("Open-Meteo has no weather for {missing} of the hike's {hours} hours")]
    Incomplete { missing: usize, hours: usize },

    #[error("Open-Meteo has no weather for {missing} of the {days} days before the hike")]
    IncompleteDays { missing: usize, days: usize },

    #[error("Open-Meteo has no weather before 1940")]
    BeforeArchive,

    #[error("Open-Meteo has no weather more than 16 days ahead")]
    BeyondForecast,
}
