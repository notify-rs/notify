use futures::stream::Stream;
use std::io;
use std::path::PathBuf;
use std::result::Result as StdResult;
use super::capability::Capability;

pub trait Backend: Stream + Drop + Sized {
    fn new(paths: Vec<PathBuf>) -> Result<Self>;
    fn capabilities() -> Vec<Capability>;
}

pub type Result<T: Backend> = StdResult<T, Error>;

pub enum Error {
    Generic(String),
    Io(io::Error),
    NotSupported(Capability),
}
