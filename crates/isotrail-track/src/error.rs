/// Why a track could not be read
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TrackError {
    /// Reading the input stream failed
    #[error("could not read the input")]
    Io(#[from] std::io::Error),

    /// The input has no XML elements at all
    #[error("the input is empty")]
    EmptyInput,

    /// The input is text without any XML
    #[error("it holds plain text, not GPX")]
    PlainText,

    /// The input is not valid UTF-8, or UTF-16 after a byte-order mark
    #[error("line {line}: the input is not valid UTF-8 or UTF-16 text")]
    Encoding {
        /// 1-based line of the first invalid byte
        line: usize,
    },

    /// The input is not well-formed XML
    #[error("line {line}: malformed XML")]
    Xml {
        /// 1-based line where the problem was found
        line: usize,
        #[source]
        source: quick_xml::Error,
    },

    /// The input ends before all its elements are closed
    #[error("line {line}: unexpected end of input, the file looks truncated")]
    Truncated {
        /// 1-based line of the end of the input
        line: usize,
    },

    /// The root element is not `<gpx>`
    #[error("it is a <{root}> file, not GPX")]
    NotGpx {
        /// Local name of the root element that was found
        root: String,
    },

    /// No segment is left after cleaning
    #[error("the track has no segment with at least two points")]
    EmptyTrack,
}
