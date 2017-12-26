use std::io;
use super::event::Event;

pub type EmptyResult = Result<(), Error>;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    UpstreamOverflow,
}

pub type Item = Event;
