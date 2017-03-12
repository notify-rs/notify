use futures::stream::Stream;
use std::{ffi, io};
use std::path::PathBuf;
use std::result::Result as StdResult;
use super::capability::Capability;

pub trait Backend: Stream + Drop + Sized {
    fn new(paths: Vec<PathBuf>) -> Result<Self>;
    fn capabilities() -> Vec<Capability>;
}

pub type Result<T: Backend> = StdResult<T, Error>;

#[derive(Debug)]
pub enum Error {
    Generic(String),
    Io(io::Error),
    NotSupported(Capability),
    FfiNul(ffi::NulError),
    FfiIntoString(ffi::IntoStringError),
    FfiFromBytes(ffi::FromBytesWithNulError)
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<Capability> for Error {
    fn from(cap: Capability) -> Self {
        Error::NotSupported(cap)
    }
}

impl From<ffi::NulError> for Error {
    fn from(err: ffi::NulError) -> Self {
        Error::FfiNul(err)
    }
}

impl From<ffi::IntoStringError> for Error {
    fn from(err: ffi::IntoStringError) -> Self {
        Error::FfiIntoString(err)
    }
}

impl From<ffi::FromBytesWithNulError> for Error {
    fn from(err: ffi::FromBytesWithNulError) -> Self {
        Error::FfiFromBytes(err)
    }
}
