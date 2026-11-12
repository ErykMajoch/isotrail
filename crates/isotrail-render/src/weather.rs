use glam::Vec2;
use isotrail_bundle::{GridInfo, Precipitation, WeatherState};

use crate::camera::slab_angle;
use crate::lighting::smoothstep;

/// Mist and fog codes in the WMO table
const FOG_CODES: [u8; 2] = [45, 48];
/// Water falling at this rate, in mm per hour, fills the frame with rain
const DOWNPOUR_MM_H: f32 = 8.0;

/// The weather as a frame draws it
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// Share of the sky under cloud, thin high cloud counting for less
    pub cloud_cover: f32,
    /// Share of the cloud low or thick enough to cast shadows
    pub solid: f32,
    /// How far cloud dims the sun and greys the sky, from 0 to 1
    pub overcast: f32,
    /// From 0 in clear air to 1 in thick fog
    pub haze: f32,
    pub precipitation: Precipitation,
    /// How thickly rain or snow falls, from 0 to 1
    pub density: f32,
    /// Where the wind blows towards, in metres per second in the slab frame
    pub wind: Vec2,
    /// Which effects a frame draws
    pub shown: WeatherEffects,
}

/// The weather's effects a frame may draw
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeatherEffects {
    /// Clouds in the sky, and the dimming and greying under them
    pub clouds: bool,
    /// The shadows of passing clouds on the ground
    pub shade: bool,
    pub rain: bool,
    pub snow: bool,
    /// Haze and fog
    pub haze: bool,
}

impl WeatherEffects {
    pub const ALL: WeatherEffects = WeatherEffects {
        clouds: true,
        shade: true,
        rain: true,
        snow: true,
        haze: true,
    };
    pub const NONE: WeatherEffects = WeatherEffects {
        clouds: false,
        shade: false,
        rain: false,
        snow: false,
        haze: false,
    };
}

impl Default for WeatherEffects {
    fn default() -> WeatherEffects {
        WeatherEffects::ALL
    }
}

/// A sky chosen for the picture instead of the recorded weather
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForcedWeather {
    Clear,
    Cloudy,
    Overcast,
    Rain,
    Snow,
    Fog,
}

impl ForcedWeather {
    pub const ALL: [ForcedWeather; 6] = [
        ForcedWeather::Clear,
        ForcedWeather::Cloudy,
        ForcedWeather::Overcast,
        ForcedWeather::Rain,
        ForcedWeather::Snow,
        ForcedWeather::Fog,
    ];

    /// The hour's weather this sky stands for, with the wind of `recorded`
    /// or, without it, a light westerly
    pub fn state(self, recorded: Option<&WeatherState>) -> WeatherState {
        let (wind_speed_m_s, wind_direction_deg) =
            recorded.map_or((3.0, 270.0), |w| (w.wind_speed_m_s, w.wind_direction_deg));
        let (weather_code, cover, low, precipitation, intensity_mm_h) = match self {
            ForcedWeather::Clear => (0, 0.0, 0.0, Precipitation::None, 0.0),
            ForcedWeather::Cloudy => (2, 50.0, 35.0, Precipitation::None, 0.0),
            ForcedWeather::Overcast => (3, 100.0, 100.0, Precipitation::None, 0.0),
            ForcedWeather::Rain => (63, 95.0, 80.0, Precipitation::Rain, 4.0),
            ForcedWeather::Snow => (73, 95.0, 80.0, Precipitation::Snow, 2.0),
            ForcedWeather::Fog => (45, 100.0, 100.0, Precipitation::None, 0.0),
        };
        let snow = self == ForcedWeather::Snow;
        WeatherState {
            temperature_c: if snow { -2.0 } else { 10.0 },
            weather_code,
            cloud_cover_pct: cover,
            cloud_low_pct: low,
            cloud_mid_pct: cover - low,
            cloud_high_pct: 0.0,
            precipitation,
            intensity_mm_h,
            snowfall_cm_h: if snow { 1.4 } else { 0.0 },
            snow_depth_m: 0.0,
            wind_speed_m_s,
            wind_direction_deg,
        }
    }
}

impl Atmosphere {
    pub fn new(grid: &GridInfo, state: &WeatherState) -> Atmosphere {
        let cover = (state.cloud_cover_pct / 100.0).clamp(0.0, 1.0);
        let low = (state.cloud_low_pct / 100.0).clamp(0.0, 1.0);
        let mid = (state.cloud_mid_pct / 100.0).clamp(0.0, 1.0);
        let high = (state.cloud_high_pct / 100.0).clamp(0.0, 1.0);
        // Thin high cloud hardly dims the sun or shades the ground
        let thick = low.max(0.8 * mid).max(0.25 * high);
        let layers = low + mid + high;
        let solid = if layers > 0.0 {
            (low + mid) / layers
        } else {
            1.0
        };
        let fog = FOG_CODES.contains(&state.weather_code);
        let falling = state.precipitation != Precipitation::None;
        let density = if falling {
            (state.intensity_mm_h / DOWNPOUR_MM_H)
                .sqrt()
                .clamp(0.15, 1.0)
        } else {
            0.0
        };
        let mut overcast = smoothstep(0.35, 1.0, thick);
        if falling || fog {
            overcast = overcast.max(0.5);
        }
        let haze =
            (if fog { 0.55 } else { 0.0 } + 0.35 * density + 0.15 * smoothstep(0.8, 1.0, low))
                .min(1.0);
        let towards = slab_angle(grid, f64::from(state.wind_direction_deg) + 180.0);
        Atmosphere {
            cloud_cover: cover.min(low + mid + 0.6 * high),
            solid: solid.min(1.0),
            overcast,
            haze,
            precipitation: state.precipitation,
            density,
            wind: Vec2::from_angle(towards) * state.wind_speed_m_s.max(0.0),
            shown: WeatherEffects::ALL,
        }
    }

    /// The same weather with only `shown` drawn
    pub fn showing(self, shown: WeatherEffects) -> Atmosphere {
        Atmosphere { shown, ..self }
    }

    /// How far cloud dims the sun and greys the sky, as drawn
    pub fn dimming(&self) -> f32 {
        if self.shown.clouds {
            self.overcast
        } else {
            0.0
        }
    }

    /// The share of the sky under cloud, as drawn
    pub fn visible_cover(&self) -> f32 {
        if self.shown.clouds {
            self.cloud_cover
        } else {
            0.0
        }
    }

    /// How hazy the air is, as drawn
    pub fn visible_haze(&self) -> f32 {
        if self.shown.haze { self.haze } else { 0.0 }
    }

    /// How deep the shade of a passing cloud is on the ground: none under a
    /// clear or an overcast sky, which dims everything evenly instead
    pub fn cloud_shadow(&self) -> f32 {
        if !self.shown.shade {
            return 0.0;
        }
        0.7 * self.solid
            * smoothstep(0.08, 0.2, self.cloud_cover)
            * smoothstep(0.95, 0.75, self.cloud_cover)
    }

    /// Rain and snow as drawn, each from 0 to 1
    pub fn rain_and_snow(&self) -> (f32, f32) {
        let (rain, snow) = match self.precipitation {
            Precipitation::None => (0.0, 0.0),
            Precipitation::Rain => (self.density, 0.0),
            Precipitation::Snow => (0.0, self.density),
            Precipitation::Mixed => (self.density / 2.0, self.density / 2.0),
        };
        (
            if self.shown.rain { rain } else { 0.0 },
            if self.shown.snow { snow } else { 0.0 },
        )
    }

    /// Whether anything drawn falls
    pub fn moves(&self) -> bool {
        let (rain, snow) = self.rain_and_snow();
        rain + snow > 0.0
    }
}

/// The share of cloud a noise value must pass to show as cloud
pub(crate) fn cloud_threshold(cover: f32) -> f32 {
    0.78 - 0.6 * cover
}

#[cfg(test)]
mod tests {
    use isotrail_testkit::grid;

    use super::*;

    fn state(code: u8, cover: f32, low: f32) -> WeatherState {
        WeatherState {
            temperature_c: 8.0,
            weather_code: code,
            cloud_cover_pct: cover,
            cloud_low_pct: low,
            cloud_mid_pct: 0.0,
            cloud_high_pct: 0.0,
            precipitation: Precipitation::None,
            intensity_mm_h: 0.0,
            snowfall_cm_h: 0.0,
            snow_depth_m: 0.0,
            wind_speed_m_s: 0.0,
            wind_direction_deg: 0.0,
        }
    }

    fn atmosphere(state: &WeatherState) -> Atmosphere {
        Atmosphere::new(&grid(10, 10, 10.0), state)
    }

    #[test]
    fn a_clear_day_changes_nothing() {
        let clear = atmosphere(&state(0, 0.0, 0.0));
        assert_eq!((clear.overcast, clear.haze, clear.density), (0.0, 0.0, 0.0));
        assert_eq!(clear.cloud_shadow(), 0.0);
        assert!(!clear.moves());
    }

    #[test]
    fn broken_cloud_casts_shadows_and_overcast_dims() {
        let partly = atmosphere(&state(2, 45.0, 30.0));
        let overcast = atmosphere(&state(3, 100.0, 100.0));
        assert!(partly.cloud_shadow() > 0.5);
        assert_eq!(overcast.cloud_shadow(), 0.0);
        assert!(partly.overcast < 0.1);
        assert_eq!(overcast.overcast, 1.0);
        let high_only = Atmosphere::new(
            &grid(10, 10, 10.0),
            &WeatherState {
                cloud_high_pct: 100.0,
                ..state(3, 100.0, 0.0)
            },
        );
        assert_eq!((high_only.overcast, high_only.cloud_shadow()), (0.0, 0.0));
        assert_eq!(high_only.cloud_cover, 0.6);
    }

    #[test]
    fn fog_codes_and_rain_bring_haze() {
        let fog = atmosphere(&state(45, 100.0, 100.0));
        assert!(fog.haze >= 0.55 && fog.overcast >= 0.5);
        let light = atmosphere(&WeatherState {
            precipitation: Precipitation::Rain,
            intensity_mm_h: 0.2,
            ..state(61, 90.0, 40.0)
        });
        let heavy = atmosphere(&WeatherState {
            precipitation: Precipitation::Rain,
            intensity_mm_h: 9.0,
            ..state(65, 90.0, 40.0)
        });
        assert!(light.density >= 0.15 && light.density < heavy.density);
        assert_eq!(heavy.density, 1.0);
        assert!(heavy.haze > light.haze && light.haze < fog.haze);
        assert_eq!(heavy.rain_and_snow(), (1.0, 0.0));
        let mixed = Atmosphere {
            precipitation: Precipitation::Mixed,
            ..heavy
        };
        assert_eq!(mixed.rain_and_snow(), (0.5, 0.5));
    }

    #[test]
    fn effects_left_out_are_not_drawn() {
        let rain = atmosphere(&ForcedWeather::Rain.state(None));
        assert!(rain.moves() && rain.dimming() > 0.0 && rain.visible_haze() > 0.0);
        assert!(rain.visible_cover() > 0.9);
        let dry = rain.showing(WeatherEffects {
            rain: false,
            ..WeatherEffects::ALL
        });
        assert!(!dry.moves() && dry.dimming() == rain.dimming());
        let none = rain.showing(WeatherEffects::NONE);
        assert_eq!(
            (none.dimming(), none.visible_cover(), none.visible_haze()),
            (0.0, 0.0, 0.0)
        );
        assert_eq!((none.rain_and_snow(), none.moves()), ((0.0, 0.0), false));
        let partly = atmosphere(&ForcedWeather::Cloudy.state(None));
        assert!(partly.cloud_shadow() > 0.5);
        let unshaded = partly.showing(WeatherEffects {
            shade: false,
            ..WeatherEffects::ALL
        });
        assert_eq!(unshaded.cloud_shadow(), 0.0);
        let no_clouds = partly.showing(WeatherEffects {
            clouds: false,
            ..WeatherEffects::ALL
        });
        assert_eq!(no_clouds.cloud_shadow(), partly.cloud_shadow());
    }

    #[test]
    fn forced_skies_read_as_named() {
        let skies = ForcedWeather::ALL.map(|sky| atmosphere(&sky.state(None)));
        let [clear, cloudy, overcast, rain, snow, fog] = skies;
        assert_eq!(
            (clear.cloud_cover, clear.overcast, clear.haze),
            (0.0, 0.0, 0.0)
        );
        assert!(cloudy.cloud_shadow() > 0.5 && cloudy.overcast < 0.1);
        assert_eq!(overcast.overcast, 1.0);
        assert!(rain.rain_and_snow().0 > 0.5 && rain.rain_and_snow().1 == 0.0);
        assert!(snow.rain_and_snow().1 > 0.3 && snow.rain_and_snow().0 == 0.0);
        assert!(fog.haze >= 0.55 && !fog.moves());
        let recorded = WeatherState {
            wind_speed_m_s: 9.0,
            wind_direction_deg: 90.0,
            ..state(0, 0.0, 0.0)
        };
        let blown = ForcedWeather::Rain.state(Some(&recorded));
        assert_eq!(
            (blown.wind_speed_m_s, blown.wind_direction_deg),
            (9.0, 90.0)
        );
    }

    #[test]
    fn the_wind_blows_away_from_where_it_comes_from() {
        for rotation_rad in [0.0, 0.7] {
            let grid = GridInfo {
                rotation_rad,
                ..grid(10, 10, 10.0)
            };
            let westerly = Atmosphere::new(
                &grid,
                &WeatherState {
                    wind_speed_m_s: 5.0,
                    wind_direction_deg: 270.0,
                    ..state(0, 0.0, 0.0)
                },
            );
            let east = Vec2::from_angle(slab_angle(&grid, 90.0)) * 5.0;
            assert!(westerly.wind.abs_diff_eq(east, 1e-4), "{:?}", westerly.wind);
        }
    }
}
