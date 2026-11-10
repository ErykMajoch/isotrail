use serde::{Deserialize, Serialize};

/// How far the year has gone on the ground, worked out when the bundle is made
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Season {
    /// Day of the year (1–366) the plants are at, counted as in the northern
    /// hemisphere; None in the tropics or for tracks without a date
    pub leaf_day: Option<u16>,
    /// Ground above this height lies under snow
    pub snow_line_m: Option<f32>,
    /// Lakes levelled above this height are frozen
    pub ice_line_m: Option<f32>,
}
