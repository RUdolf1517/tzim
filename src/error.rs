use std::fmt;
use std::io;

/// Every failure in Tzim is one of these. There is deliberately no error-chain
/// crate: the categories are few and they map directly onto exit codes and onto
/// the `kind` field of the JSON output.
#[derive(Debug)]
pub enum Error {
    /// Filesystem or other OS failure.
    Io(io::Error),
    /// The input could not be parsed with full confidence.
    Parse(String),
    /// A configured limit was exceeded (size, pixels, ratio, ...).
    Limit(String),
    /// A candidate violated its contract. This is a bug in Tzim.
    Verify(String),
    /// Invalid command line.
    Usage(String),
    /// An internal invariant was broken.
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Parse(m) => write!(f, "{m}"),
            Error::Limit(m) => write!(f, "{m}"),
            Error::Verify(m) => write!(f, "verification failed: {m}"),
            Error::Usage(m) => write!(f, "{m}"),
            Error::Internal(m) => write!(f, "internal error: {m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl Error {
    /// Stable identifier used in the JSON output and in logs.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Io(_) => "io",
            Error::Parse(_) => "parse",
            Error::Limit(_) => "limit",
            Error::Verify(_) => "verify",
            Error::Usage(_) => "usage",
            Error::Internal(_) => "internal",
        }
    }

    /// A verification failure aborts the whole run: it means a contract was
    /// violated, which is a bug rather than an unreadable input file.
    pub fn is_verify(&self) -> bool {
        matches!(self, Error::Verify(_))
    }

    pub fn parse(msg: impl Into<String>) -> Self {
        Error::Parse(msg.into())
    }

    pub fn verify(msg: impl Into<String>) -> Self {
        Error::Verify(msg.into())
    }

    pub fn limit(msg: impl Into<String>) -> Self {
        Error::Limit(msg.into())
    }

    pub fn usage(msg: impl Into<String>) -> Self {
        Error::Usage(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Error::Internal(msg.into())
    }
}
