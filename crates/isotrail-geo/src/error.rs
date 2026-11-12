/// Why geographic preparation failed
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GeoError {
    /// There is nothing to frame
    #[error("the track has no points")]
    EmptyTrack,
    /// A route on one spot with no padding around it
    #[error("the route stays on one spot and the padding adds no ground around it")]
    NoGround,
    #[error(transparent)]
    Track(#[from] isotrail_track::TrackError),
}
