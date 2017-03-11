use std::io;
use super::event::Event;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    UpstreamOverflow,
}

pub type Item = Event;
