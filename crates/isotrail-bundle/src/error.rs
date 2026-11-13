/// Why a bundle could not be read or written
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BundleError {
    #[error("not an isotrail scene bundle")]
    NotABundle,
    #[error(
        "the bundle was made by an older isotrail (format version {found}; this one reads \
         versions {oldest} and {supported}): create it again from the GPX with `isotrail bundle`"
    )]
    OlderVersion {
        found: u16,
        oldest: u16,
        supported: u16,
    },
    #[error(
        "the bundle was made by a newer isotrail (format version {found}; this one reads \
         version {supported}): update isotrail to open it"
    )]
    NewerVersion { found: u16, supported: u16 },
    #[error("the bundle is damaged: {0}")]
    Corrupt(String),
    #[error("the scene can't be saved as a bundle: {0}")]
    Invalid(String),
}
