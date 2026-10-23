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

    /// The input is not valid UTF-8
    #[error("line {line}: the input is not valid UTF-8")]
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
    #[error("not a GPX file: the root element is <{root}>")]
    NotGpx {
        /// Local name of the root element that was found
        root: String,
    },

    /// A required attribute is missing
    #[error("line {line}: <{element}> has no `{attribute}` attribute")]
    MissingAttribute {
        /// 1-based line of the element
        line: usize,
        element: &'static str,
        attribute: &'static str,
    },

    /// A value is not a valid number or time, or is out of range
    #[error("line {line}: invalid {field} {value:?}")]
    InvalidValue {
        /// 1-based line of the value
        line: usize,
        /// What the value describes e.g. `"latitude"`
        field: &'static str,
        /// The value as written in the file
        value: String,
    },

    /// No segment is left after cleaning
    #[error("the track has no segment with at least two points")]
    EmptyTrack,
}
