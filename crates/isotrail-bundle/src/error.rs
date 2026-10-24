/// Why a bundle could not be read
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BundleError {
    #[error("not an isotrail scene bundle")]
    NotABundle,
    #[error(
        "the bundle uses format version {found}, but this isotrail reads version {supported}; \
         create it again with `isotrail bundle`"
    )]
    UnsupportedVersion { found: u16, supported: u16 },
    #[error("the bundle is damaged: {0}")]
    Corrupt(String),
}
