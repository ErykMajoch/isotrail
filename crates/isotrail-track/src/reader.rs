use std::io::Read;

use crate::{Track, TrackError};

pub trait TrackReader {
    fn parse(&self, input: &[u8]) -> Result<Track, TrackError>;

    fn read(&self, input: &mut dyn Read) -> Result<Track, TrackError> {
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes)?;
        self.parse(&bytes)
    }
}
