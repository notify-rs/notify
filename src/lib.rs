#![feature(phase)]
#[phase(plugin, link)] extern crate log;

use std::io::IoError;

#[cfg(target_os="linux")]
pub mod inotify;

#[deriving(Send)]
pub struct Event {
  pub path: Path,
  pub op: Op,
}

bitflags! {
  flags Op: u32 {
    const CREATE  = 0x00000001,
    const WRITE   = 0x00000010,
    const REMOVE  = 0x00000100,
    const RENAME  = 0x00001000,
    const CHMOD   = 0x00010000,
  }
}

pub enum Error {
  Generic(String),
  Io(IoError),
  NotImplemented,
}

pub trait Watcher {
  fn new(Sender<Event>) -> Result<Self, Error>;
  fn watch(&mut self, &Path) -> Result<(), Error>;
  fn unwatch(&mut self, &Path) -> Result<(), Error>;
}
