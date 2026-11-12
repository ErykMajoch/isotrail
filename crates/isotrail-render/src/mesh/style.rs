use isotrail_bundle::Landcover;

use super::{SIDE_SHADE, cover, props, season, strata};

/// The colours of the slab and its props, and how often their details show
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlabStyle {
    /// One colour per class, in [`Landcover::ALL`] order
    pub landcover: [[u8; 4]; 13],
    /// How much darker the walls of the terrain are than its tops
    pub side_shade: f32,
    pub strata: StrataStyle,
    pub props: PropStyle,
    pub water: WaterStyle,
    pub paths: PathStyle,
    pub season: SeasonStyle,
}

impl Default for SlabStyle {
    fn default() -> SlabStyle {
        SlabStyle {
            landcover: Landcover::ALL.map(|class| {
                let [r, g, b] = class.legend_rgb();
                [r, g, b, 255]
            }),
            side_shade: SIDE_SHADE,
            strata: StrataStyle::default(),
            props: PropStyle::default(),
            water: WaterStyle::default(),
            paths: PathStyle::default(),
            season: SeasonStyle::default(),
        }
    }
}

impl SlabStyle {
    /// A class's colour, magenta for a value no class has
    pub fn landcover(&self, class: u8) -> [u8; 4] {
        self.landcover
            .get(usize::from(class))
            .copied()
            .unwrap_or([255, 0, 255, 255])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrataStyle {
    pub water: [u8; 4],
    pub sea_water: [u8; 4],
    pub dirt: [u8; 4],
    pub pebble: [u8; 4],
    pub stone: [u8; 4],
    pub deep_stone: [u8; 4],
    pub copper: [u8; 4],
    pub gold: [u8; 4],
    pub crystal: [u8; 4],
    /// Specks per thousand blocks of dirt, stone and deep stone
    pub specks_per_mille: [u32; 3],
}

impl Default for StrataStyle {
    fn default() -> StrataStyle {
        StrataStyle {
            water: strata::WATER,
            sea_water: strata::SEA_WATER,
            dirt: strata::DIRT,
            pebble: strata::PEBBLE,
            stone: strata::STONE,
            deep_stone: strata::DEEP_STONE,
            copper: strata::COPPER,
            gold: strata::GOLD,
            crystal: strata::CRYSTAL,
            specks_per_mille: strata::SPECKS_PER_MILLE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropStyle {
    pub needles: [u8; 4],
    pub leaves: [u8; 4],
    pub bush: [u8; 4],
    pub trunk: [u8; 4],
    pub boulder: [u8; 4],
    pub twigs: [u8; 4],
    pub bare_bush: [u8; 4],
    /// Internal pixels between neighbouring spots where a prop may stand
    pub spacing_px: f32,
    /// How far a prop may stray from its spot, as a share of the spacing
    pub jitter: f32,
}

impl Default for PropStyle {
    fn default() -> PropStyle {
        PropStyle {
            needles: props::NEEDLES,
            leaves: props::LEAVES,
            bush: props::BUSH,
            trunk: props::TRUNK,
            boulder: props::BOULDER,
            twigs: props::TWIGS,
            bare_bush: props::BARE_BUSH,
            spacing_px: props::SPACING_PX,
            jitter: props::JITTER,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterStyle {
    pub shore: [u8; 4],
    pub sea: [u8; 4],
    pub foam: [u8; 4],
    pub glint: [u8; 4],
    pub ice: [u8; 4],
    pub ice_crack: [u8; 4],
    pub glint_per_mille: u32,
    pub crack_per_mille: u32,
    /// Foam on the sea's cells against the land, and on the ring beyond them
    pub foam_percent: [u32; 2],
}

impl Default for WaterStyle {
    fn default() -> WaterStyle {
        WaterStyle {
            shore: cover::SHORE,
            sea: cover::SEA,
            foam: cover::FOAM,
            glint: cover::GLINT,
            ice: season::ICE,
            ice_crack: season::CRACK,
            glint_per_mille: cover::GLINT_PER_MILLE,
            crack_per_mille: season::CRACK_PER_MILLE,
            foam_percent: cover::FOAM_PERCENT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathStyle {
    pub trodden: [u8; 4],
    pub paved: [u8; 4],
    /// How far paths and roads turn the ground towards their colours
    pub path_blend: f32,
    pub road_blend: f32,
}

impl Default for PathStyle {
    fn default() -> PathStyle {
        PathStyle {
            trodden: cover::TRODDEN,
            paved: cover::PAVED,
            path_blend: cover::PATH_BLEND,
            road_blend: cover::ROAD_BLEND,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeasonStyle {
    pub snow: [u8; 4],
    pub fresh: [u8; 4],
    pub blossom: [[u8; 4]; 2],
    pub autumn: [[u8; 4]; 4],
    pub litter: [u8; 4],
    pub flowers: [[u8; 4]; 3],
    pub flowers_per_mille: u32,
    pub young_crop: [u8; 4],
    pub golden: [u8; 4],
    pub ploughed: [u8; 4],
    pub bracken: [u8; 4],
    pub dull: [u8; 4],
    pub dark_heath: [u8; 4],
}

impl Default for SeasonStyle {
    fn default() -> SeasonStyle {
        SeasonStyle {
            snow: season::SNOW,
            fresh: season::FRESH,
            blossom: season::BLOSSOM,
            autumn: season::AUTUMN,
            litter: season::LITTER,
            flowers: season::FLOWERS,
            flowers_per_mille: season::FLOWER_PER_MILLE,
            young_crop: season::YOUNG_CROP,
            golden: season::GOLDEN,
            ploughed: season::PLOUGHED,
            bracken: season::BRACKEN,
            dull: season::DULL,
            dark_heath: season::DARK_HEATH,
        }
    }
}
