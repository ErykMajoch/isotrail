# Recorded weather data

`archive-api.open-meteo.com/v1/archive-db9205666f7acb0c` is the [Open-Meteo historical weather API](https://open-meteo.com/en/docs/historical-weather-api) answer for the synthetic route in `../gpx/snowdon.gpx`. It holds hourly values for 14 June 2025 (UTC) at one point in the middle of the route, downscaled to the route's mean height of 830m.

`archive-api.open-meteo.com/v1/archive-845f6c2effc8cda6` holds the daily low, high and snowfall at the same point for the fortnight before, from 31 May to 13 June 2025. Both were recorded on 2026-11-10 and are stored as received.

The folders follow the request's address, and each file name ends in a hash of its query, so tests can serve it with `RecordedFetcher`. When a request changes, so does its hash. The failing test then names the file it expected, and the answer has to be recorded again.

[Weather data by Open-Meteo.com](https://open-meteo.com/), available under the [Creative Commons Attribution 4.0 International licence](https://creativecommons.org/licenses/by/4.0/).
