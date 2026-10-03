use std::{fmt, io};

use needle_core::header::HeaderError;
use needle_core::outline::OutlineError;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// A file exists but can't be understood, or an operation doesn't make sense.
    Invalid(String),
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Invalid(e) | Self::NotFound(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<OutlineError> for Error {
    fn from(e: OutlineError) -> Self {
        match e {
            OutlineError::Invalid(_) => Self::Invalid(e.to_string()),
            OutlineError::NoSuchChapter(_) | OutlineError::NoSuchScene(_) => Self::NotFound(e.to_string()),
        }
    }
}

impl From<HeaderError> for Error {
    fn from(e: HeaderError) -> Self {
        Self::Invalid(e.to_string())
    }
}
