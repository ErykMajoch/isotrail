//! Format 5, the layout before water bodies had a sea flag. Only the bundle
//! and its water bodies differ, so the other types are today's

use serde::Deserialize;

use crate::{
    GridInfo, Metadata, MoonSample, ProfileSample, Route, Season, SunSample, Terrain, Weather,
};

pub const FORMAT_VERSION: u16 = 5;

#[derive(Deserialize)]
pub struct SceneBundle {
    meta: Metadata,
    grid: GridInfo,
    terrain: Terrain,
    water: Vec<WaterBody>,
    prop_seed: u64,
    route: Route,
    profile: Vec<ProfileSample>,
    sun: Vec<SunSample>,
    moon: Vec<MoonSample>,
    weather: Option<Weather>,
    season: Season,
}

#[derive(Deserialize)]
struct WaterBody {
    level_m: f32,
    cells: Vec<u32>,
}

impl From<SceneBundle> for crate::SceneBundle {
    fn from(old: SceneBundle) -> Self {
        let mut terrain = old.terrain;
        // Format 5 gave flat ground a height step of a whole metre, more
        // than today's check allows; every height is 0 there, so the step
        // changes nothing
        if terrain.heights.iter().all(|&h| h == 0) {
            terrain.height_scale_m = 1.0 / f32::from(u16::MAX);
        }
        crate::SceneBundle {
            meta: old.meta,
            grid: old.grid,
            terrain,
            water: old
                .water
                .into_iter()
                .map(|body| crate::WaterBody {
                    level_m: body.level_m,
                    cells: body.cells,
                    sea: false,
                })
                .collect(),
            prop_seed: old.prop_seed,
            route: old.route,
            profile: old.profile,
            sun: old.sun,
            moon: old.moon,
            weather: old.weather,
            season: old.season,
        }
    }
}
