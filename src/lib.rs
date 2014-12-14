#![feature(phase)]
#[phase(plugin, link)] extern crate log;

use std::io::IoError;
pub use self::op::Op;
pub use self::inotify::INotifyWatcher;
pub use self::poll::PollWatcher;

#[cfg(target_os="linux")]
pub mod inotify;
pub mod poll;

pub mod op {
  bitflags! {
    #[deriving(Copy)] flags Op: u32 {
      const CHMOD   = 0b00001,
      const CREATE  = 0b00010,
      const REMOVE  = 0b00100,
      const RENAME  = 0b01000,
      const WRITE   = 0b10000,
    }
  }
}

#[deriving(Send)]
pub struct Event {
  pub path: Option<Path>,
  pub op: Result<Op, Error>,
}

pub enum Error {
  Generic(String),
  Io(IoError),
  NotImplemented,
  PathNotFound,
  WatchNotFound,
}

pub trait Watcher {
  fn new(Sender<Event>) -> Result<Self, Error>;
  fn watch(&mut self, &Path) -> Result<(), Error>;
  fn unwatch(&mut self, &Path) -> Result<(), Error>;
}

#[cfg(target_os = "linux")]
pub fn new(tx: Sender<Event>) -> Result<INotifyWatcher, Error> {
  Watcher::new(tx)
}

#[cfg(not(any(target_os = "linux")))]
pub fn new(tx: Sender<Event>) -> Result<PollWatcher, Error> {
  Watcher::new(tx)
}
