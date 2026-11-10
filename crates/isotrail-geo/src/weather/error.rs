use crate::FetchError;

/// Why the weather could not be found
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WeatherError {
    /// The request failed, or offline mode is on and it isn't cached
    #[error("could not fetch the weather from Open-Meteo")]
    Fetch {
        #[source]
        source: FetchError,
    },

    #[error("could not read the Open-Meteo response")]
    Json {
        #[source]
        source: serde_json::Error,
    },

    /// Some hours are missing or have gaps, as for hikes still in progress
    #[error("Open-Meteo has no weather for {missing} of the hike's {hours} hours")]
    Incomplete { missing: usize, hours: usize },
}
