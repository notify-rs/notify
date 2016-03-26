#[macro_use] extern crate log;
#[macro_use] extern crate bitflags;
#[cfg(target_os="linux")] extern crate mio;
#[cfg(target_os="macos")] extern crate fsevent_sys;
#[cfg(target_os="windows")] extern crate winapi;
extern crate libc;
extern crate filetime;

pub use self::op::Op;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::convert::AsRef;
use std::fmt;
// Permits retrieve err.description()
use std::error::Error as StdError;

#[cfg(target_os="macos")] pub use self::fsevent::FsEventWatcher;
#[cfg(target_os="linux")] pub use self::inotify::INotifyWatcher;
#[cfg(target_os="windows")] pub use self::windows::ReadDirectoryChangesWatcher;
pub use self::null::NullWatcher;
pub use self::poll::PollWatcher;

#[cfg(target_os="linux")] pub mod inotify;
#[cfg(target_os="macos")] pub mod fsevent;
#[cfg(target_os="windows")] pub mod windows;
pub mod null;
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

#[derive(Debug)]
pub struct Event {
  pub path: Option<PathBuf>,
  pub op: Result<Op, Error>,
}

unsafe impl Send for Event {}

#[derive(Debug)]
pub enum Error {
  Generic(String),
  Io(io::Error),
  NotImplemented,
  PathNotFound,
  WatchNotFound,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let error = String::from(match *self {
            Error::PathNotFound => "No path was found.",
            Error::WatchNotFound => "No watch was found.",
            Error::NotImplemented => "Not implemented.",
            Error::Generic(ref err) => err.as_ref(),
            Error::Io(ref err) => err.description(),
        });

        write!(f, "{}", error)
    }
}

pub trait Watcher: Sized {
  fn new(Sender<Event>) -> Result<Self, Error>;
  fn watch<P: AsRef<Path>>(&mut self, P) -> Result<(), Error>;
  fn unwatch<P: AsRef<Path>>(&mut self, P) -> Result<(), Error>;
}

#[cfg(target_os = "linux")] pub type RecommendedWatcher = INotifyWatcher;
#[cfg(target_os = "macos")] pub type RecommendedWatcher = FsEventWatcher;
#[cfg(target_os = "windows")] pub type RecommendedWatcher = ReadDirectoryChangesWatcher;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))] pub type RecommendedWatcher = PollWatcher;

pub fn new(tx: Sender<Event>) -> Result<RecommendedWatcher, Error> {
  Watcher::new(tx)
}


#[test]
fn display_formatted_errors() {
    let expected = "Some error";
    assert_eq!(expected,
               format!("{}", Error::Generic(String::from(expected))));
    assert_eq!(
        expected,
        format!("{}",
            Error::Io(io::Error::new(io::ErrorKind::Other, expected))
            )
        );
}
