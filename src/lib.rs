#![feature(plugin)]
#![allow(unstable)]

#[plugin] extern crate log;
#[macro_use] extern crate bitflags;

use std::io::IoError;
use std::sync::mpsc::Sender;
#[cfg(test)] use std::sync::mpsc::channel;
pub use self::op::Op;
#[cfg(target_os="linux")]
pub use self::inotify::INotifyWatcher;
pub use self::poll::PollWatcher;

#[cfg(target_os="linux")]
pub mod inotify;
pub mod poll;

pub mod op {
  bitflags! {
    flags Op: u32 {
      const CHMOD   = 0b00001,
      const CREATE  = 0b00010,
      const REMOVE  = 0b00100,
      const RENAME  = 0b01000,
      const WRITE   = 0b10000,
    }
  }
}

pub struct Event {
  pub path: Option<Path>,
  pub op: Result<Op, Error>,
}

unsafe impl Send for Event {}

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

#[cfg(target_os = "linux")] pub type RecommendedWatcher = INotifyWatcher;
#[cfg(not(any(target_os = "linux")))] pub type RecommendedWatcher = PollWatcher;

pub fn new(tx: Sender<Event>) -> Result<RecommendedWatcher, Error> {
  Watcher::new(tx)
}

#[test]
#[cfg(target_os = "linux")]
fn new_inotify() {
  let (tx, _) = channel();
  let w: Result<INotifyWatcher, Error> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[test]
fn new_poll() {
  let (tx, _) = channel();
  let w: Result<PollWatcher, Error> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[test]
fn new_recommended() {
  let (tx, _) = channel();
  let w: Result<RecommendedWatcher, Error> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}
