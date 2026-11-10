# Recorded weather data

`archive-api.open-meteo.com/v1/archive-db9205666f7acb0c` is the [Open-Meteo historical weather API](https://open-meteo.com/en/docs/historical-weather-api) response for the synthetic route in `../gpx/snowdon.gpx`: hourly values for 14 June 2025 (UTC) at one point in the middle of the route, downscaled to the route's mean height of 830 m, recorded on 2026-11-10. The directory layout mirrors the request URL, and the file name ends in a hash of the query string, so tests can serve it with `RecordedFetcher`. If the request changes, the hash changes too: the failing test names the file it expected, and the response has to be recorded again. The response is stored as received.

[Weather data by Open-Meteo.com](https://open-meteo.com/), available under the [Creative Commons Attribution 4.0 International licence](https://creativecommons.org/licenses/by/4.0/).
