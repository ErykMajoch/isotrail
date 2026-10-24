/// Why geographic preparation failed
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GeoError {
    /// There is nothing to frame
    #[error("the track has no points")]
    EmptyTrack,
}
