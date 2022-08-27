use std::{fmt, io};

mod router;
pub use router::BaseRouter;
mod public;
pub use public::{PublicOr, PublicOrLayer};
mod fallback;
pub use fallback::FallbackService;

#[derive(Debug)]
pub enum Error {
    Hyper(hyper::Error),
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<hyper::Error> for Error {
    fn from(e: hyper::Error) -> Self {
        Self::Hyper(e)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Hyper(e) => Some(e),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => fmt::Display::fmt(e, f),
            Self::Hyper(e) => fmt::Display::fmt(e, f),
        }
    }
}
