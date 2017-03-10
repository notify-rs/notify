use std::io;
use super::event::Event;

pub enum Error {
    Io(io::Error),
    UpstreamOverflow,
}

pub type Item = Event;
