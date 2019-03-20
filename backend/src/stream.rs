//! The types related to implementing the `Stream` trait.

use super::event::Event;
use std::{
    io, num::NonZeroU16, sync::Arc,
    hash::{Hash, Hasher},
};

/// A specialised error for `Backend::await()`.
pub type EmptyResult = Result<(), Error>;

/// Any error which may occur while pulling an event stream.
#[derive(Clone, Debug)]
pub enum Error {
    /// An I/O error.
    Io(Arc<io::Error>),

    /// An error representing when the backend's upstream has overflowed.
    UpstreamOverflow,

    /// An error indicating that some amount of events are missing.
    ///
    /// This is used when a queue or stream fills to capacity and no further events can be
    /// buffered. In those cases, a Missed error may be issued, along with an optional hint of how
    /// many events were dropped, if that is known or can be estimated.
    ///
    /// The absence of this error in a stream does not mean that no events were dropped.
    ///
    /// The count may be inaccurate. It is a hint only. Do not rely on its exact value.
    Missed(Option<NonZeroU16>),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(Arc::new(err))
    }
}

impl Eq for Error {}
impl PartialEq<Error> for Error {
    fn eq(&self, other: &Error) -> bool {
        match (self, other) {
            (Error::Io(_), _) => false,
            (_, Error::Io(_)) => false,
            (a, b) => a == b,
        }
    }
}

impl Hash for Error {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Error::Io(aerr) => aerr.raw_os_error().hash(state),
            rest => rest.hash(state),
        };
    }
}

// TODO: impl Display and Error

/// A handy reference to the correct `Stream::Item` associated type.
pub type Item = Result<Event, Error>;
