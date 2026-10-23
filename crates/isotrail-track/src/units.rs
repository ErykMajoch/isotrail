const METRES_PER_MILE: f64 = 1609.344;
const METRES_PER_FOOT: f64 = 0.3048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Units {
    #[default]
    Metric,
    Imperial,
}

impl Units {
    /// Kilometres or miles
    pub fn distance(self, metres: f64) -> f64 {
        match self {
            Units::Metric => metres / 1000.0,
            Units::Imperial => metres / METRES_PER_MILE,
        }
    }

    pub fn distance_unit(self) -> &'static str {
        match self {
            Units::Metric => "km",
            Units::Imperial => "mi",
        }
    }

    /// Metres or feet
    pub fn elevation(self, metres: f64) -> f64 {
        match self {
            Units::Metric => metres,
            Units::Imperial => metres / METRES_PER_FOOT,
        }
    }

    pub fn elevation_unit(self) -> &'static str {
        match self {
            Units::Metric => "m",
            Units::Imperial => "ft",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(Units::Metric.distance(12_340.0), 12.34);
        assert!((Units::Imperial.distance(1609.344) - 1.0).abs() < 1e-12);
        assert_eq!(Units::Metric.elevation(1000.0), 1000.0);
        assert!((Units::Imperial.elevation(1000.0) - 3280.84).abs() < 0.01);
        assert_eq!(Units::default(), Units::Metric);
        assert_eq!(
            (
                Units::Imperial.distance_unit(),
                Units::Imperial.elevation_unit()
            ),
            ("mi", "ft")
        );
    }
}
