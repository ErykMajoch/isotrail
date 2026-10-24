pub const EXAGGERATION_FACTOR: f64 = 0.15;
pub const MIN_EXAGGERATION: f64 = 1.0;
pub const MAX_EXAGGERATION: f64 = 5.0;

/// Flat terrain gets the maximum, so gentle relief still reads as a landscape
pub fn vertical_exaggeration(long_side_m: f64, relief_m: f64) -> f64 {
    let exaggeration = EXAGGERATION_FACTOR * long_side_m / relief_m;
    if relief_m > 0.0 && exaggeration.is_finite() {
        exaggeration.clamp(MIN_EXAGGERATION, MAX_EXAGGERATION)
    } else {
        MAX_EXAGGERATION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_formula_between_the_clamps() {
        assert!((vertical_exaggeration(10_000.0, 500.0) - 3.0).abs() < 1e-12);
        assert!((vertical_exaggeration(8_000.0, 600.0) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn clamps_to_one_for_alpine_relief() {
        assert_eq!(vertical_exaggeration(6_000.0, 1_500.0), MIN_EXAGGERATION);
        assert_eq!(vertical_exaggeration(10_000.0, 1_500.0), MIN_EXAGGERATION);
    }

    #[test]
    fn clamps_to_five_for_flat_terrain() {
        assert_eq!(vertical_exaggeration(40_000.0, 100.0), MAX_EXAGGERATION);
        assert_eq!(vertical_exaggeration(10_000.0, 0.0), MAX_EXAGGERATION);
        assert_eq!(vertical_exaggeration(10_000.0, -5.0), MAX_EXAGGERATION);
        assert_eq!(vertical_exaggeration(f64::NAN, 100.0), MAX_EXAGGERATION);
    }
}
