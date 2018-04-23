//! The types related to implementing the `Stream` trait.

use futures::sync::mpsc::SendError;
use std::io;
use super::event::Event;

/// A specialised error for `Backend::await()`.
pub type EmptyResult = Result<(), Error>;

/// Any error which may occur while pulling an event stream.
#[derive(Debug)]
pub enum Error {
    /// An I/O error.
    Io(io::Error),

    /// An MPSC futures channel error.
    Mpsc(SendError<Event>),

    /// An error representing when the backend's upstream has overflowed.
    UpstreamOverflow,
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<SendError<Event>> for Error {
    fn from(err: SendError<Event>) -> Self {
        Error::Mpsc(err)
    }
}

/// A handy reference to the correct `Stream::Item` associated type.
pub type Item = Event;
