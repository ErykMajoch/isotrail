/// What covers the ground in a grid cell; the numbers are stored in bundles,
/// so they never change
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Landcover {
    Water = 0,
    Snow = 1,
    Rock = 2,
    Scree = 3,
    Grassland = 4,
    Heath = 5,
    Scrub = 6,
    Coniferous = 7,
    Broadleaf = 8,
    MixedForest = 9,
    Wetland = 10,
    Farmland = 11,
    BuiltUp = 12,
}

impl Landcover {
    pub const ALL: [Landcover; 13] = [
        Landcover::Water,
        Landcover::Snow,
        Landcover::Rock,
        Landcover::Scree,
        Landcover::Grassland,
        Landcover::Heath,
        Landcover::Scrub,
        Landcover::Coniferous,
        Landcover::Broadleaf,
        Landcover::MixedForest,
        Landcover::Wetland,
        Landcover::Farmland,
        Landcover::BuiltUp,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Landcover::Water => "water",
            Landcover::Snow => "snow and ice",
            Landcover::Rock => "bare rock",
            Landcover::Scree => "scree",
            Landcover::Grassland => "grassland",
            Landcover::Heath => "heath and moorland",
            Landcover::Scrub => "scrub",
            Landcover::Coniferous => "coniferous forest",
            Landcover::Broadleaf => "broadleaf forest",
            Landcover::MixedForest => "mixed forest",
            Landcover::Wetland => "wetland",
            Landcover::Farmland => "farmland",
            Landcover::BuiltUp => "built-up",
        }
    }

    pub fn is_forest(self) -> bool {
        matches!(
            self,
            Landcover::Coniferous | Landcover::Broadleaf | Landcover::MixedForest
        )
    }
}

impl From<Landcover> for u8 {
    fn from(class: Landcover) -> u8 {
        class as u8
    }
}

/// Fails with the unknown value
impl TryFrom<u8> for Landcover {
    type Error = u8;

    fn try_from(value: u8) -> Result<Landcover, u8> {
        Landcover::ALL.get(usize::from(value)).copied().ok_or(value)
    }
}

/// Bits of the per-cell line mask, for ways too narrow to be a landcover
pub mod line_mask {
    pub const PATH: u8 = 1;
    pub const ROAD: u8 = 1 << 1;
    pub const STREAM: u8 = 1 << 2;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_stable_and_round_trip() {
        assert_eq!(u8::from(Landcover::Water), 0);
        assert_eq!(u8::from(Landcover::MixedForest), 9);
        assert_eq!(u8::from(Landcover::BuiltUp), 12);
        for (i, class) in Landcover::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(u8::from(class)), i);
            assert_eq!(Landcover::try_from(u8::from(class)), Ok(class));
        }
        assert_eq!(Landcover::try_from(13), Err(13));
        assert_eq!(Landcover::try_from(255), Err(255));
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = Landcover::ALL.map(Landcover::name).to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Landcover::ALL.len());
    }
}
