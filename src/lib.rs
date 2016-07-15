//! Cross-platform file system notification library
//!
//! The source code for this project can be found on [GitHub](https://github.com/passcod/rsnotify).
//!
//! # Installation
//!
//! Simply add `notify` to your _Cargo.toml_.
//!
//! ```toml
//! [dependencies]
//! notify = "^2.5.0"
//! ```
//!
//! # Examples
//!
//! Basic usage
//!
//! ```no_run
//! extern crate notify;
//!
//! use notify::{RecommendedWatcher, Error, Watcher, RecursiveMode};
//! use std::sync::mpsc::channel;
//!
//! fn main() {
//!   // Create a channel to receive the events.
//!   let (tx, rx) = channel();
//!
//!   // Automatically select the best implementation for your platform.
//!   // You can also access each implementation directly e.g. INotifyWatcher.
//!   let w: Result<RecommendedWatcher, Error> = Watcher::new(tx);
//!
//!   match w {
//!     Ok(mut watcher) => {
//!       // Add a path to be watched. All files and directories at that path and
//!       // below will be monitored for changes.
//!       watcher.watch("/home/test/notify", RecursiveMode::Recursive);
//!
//!       // You'll probably want to do that in a loop. The type to match for is
//!       // notify::Event, look at src/lib.rs for details.
//!       match rx.recv() {
//!         _ => println!("Recv.")
//!       }
//!     },
//!     Err(_) => println!("Error")
//!   }
//! }
//! ```
//!
//! ## Platforms
//!
//! - Linux / Android: inotify
//! - OS X: FSEvent
//! - Windows: ReadDirectoryChangesW
//! - All platforms: polling
//!
//! ## Limitations
//!
//! ### FSEvent
//!
//! Due to the inner security model of FSEvent (see
//! [FileSystemEventSecurity](https://developer.apple.com/library/mac/documentation/Darwin/Conceptual/FSEvents_ProgGuide/FileSystemEventSecurity/FileSystemEventSecurity.html)),
//! some event cannot be observed easily when trying to follow files that do not belong to you. In
//! this case, reverting to the pollwatcher can fix the issue, with a slight performance cost.
//!
//! ## Todo
//!
//! - BSD / OS X / iOS: kqueue
//! - Solaris 11: FEN
//!
//! Pull requests and bug reports happily accepted!
//!
//! ## Origins
//!
//! Inspired by Go's [fsnotify](https://github.com/go-fsnotify/fsnotify), born out
//! of need for [cargo watch](https://github.com/passcod/cargo-watch), and general
//! frustration at the non-existence of C/Rust cross-platform notify libraries.
//!
//! Written by [Félix Saparelli](https://passcod.name) and awesome
//! [contributors](https://github.com/passcod/rsnotify/graphs/contributors),
//! and released in the Public Domain using the Creative Commons Zero Declaration.

#![deny(missing_docs)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate bitflags;
#[cfg(target_os="linux")]
extern crate mio;
#[cfg(target_os="macos")]
extern crate fsevent_sys;
#[cfg(target_os="windows")]
extern crate winapi;
extern crate libc;
extern crate filetime;

pub use self::op::Op;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::convert::AsRef;
use std::fmt;
use std::error::Error as StdError;
use std::result::Result as StdResult;

#[cfg(target_os="macos")]
pub use self::fsevent::FsEventWatcher;
#[cfg(target_os="linux")]
pub use self::inotify::INotifyWatcher;
#[cfg(target_os="windows")]
pub use self::windows::ReadDirectoryChangesWatcher;
pub use self::null::NullWatcher;
pub use self::poll::PollWatcher;

#[cfg(target_os="linux")]
pub mod inotify;
#[cfg(target_os="macos")]
pub mod fsevent;
#[cfg(target_os="windows")]
pub mod windows;

pub mod null;
pub mod poll;

/// Contains the Op type which describes the actions for which an event is delivered
pub mod op {
    bitflags! {
        /// Detected actions for which an Event is delivered
        ///
        /// Multiple actions may be delivered in a single event.
        pub flags Op: u32 {
            /// Permissions changed
            const CHMOD   = 0b000001,
            /// Created
            const CREATE  = 0b000010,
            /// Removed
            const REMOVE  = 0b000100,
            /// Renamed
            const RENAME  = 0b001000,
            /// Written
            const WRITE   = 0b010000,
            /// Watch has been ignored by the implementation
            const IGNORED = 0b100000,
        }
    }
}

/// Event delivered when action occurs on a watched path
///
/// When using the poll watcher, `op` may be Err in the case where getting metadata for the path
/// fails.
///
/// When using the `INotifyWatcher`, `op` may be Err if activity is detected on the file and there is
/// an error reading from inotify.
#[derive(Debug)]
pub struct Event {
    /// Path where `Event` originated
    pub path: Option<PathBuf>,

    /// Operation detected on Path
    pub op: Result<Op>,

    /// Unique cookie associating related rename events
    pub cookie: Option<u32>,
}

unsafe impl Send for Event {}

/// Errors generated from the `notify` crate
#[derive(Debug)]
pub enum Error {
    /// Generic error
    ///
    /// May be used in cases where a platform specific error is mapped to this type
    Generic(String),

    /// I/O errors
    Io(io::Error),

    /// Something isn't implemented in notify
    ///
    /// TODO this isn't used and should be removed
    NotImplemented,

    /// The provided path does not exist
    PathNotFound,

    /// Attempted to remove a watch that does not exist
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

/// Type alias to use this library's `Error` type in a Result
pub type Result<T> = StdResult<T, Error>;

impl StdError for Error {
    fn description(&self) -> &str {
        match *self {
            Error::PathNotFound => "No path was found",
            Error::WatchNotFound => "No watch was found",
            Error::NotImplemented => "Not implemented",
            Error::Generic(_) => "Generic error",
            Error::Io(_) => "I/O Error",
        }
    }

    fn cause(&self) -> Option<&StdError> {
        match *self {
            Error::Io(ref cause) => Some(cause),
            _ => None
        }
    }
}

/// Indicates whether only the provided directory or its sub-directories as well should be watched
#[derive(Debug)]
pub enum RecursiveMode {
    /// Watch all sub-directories as well, including directories created after installing the watch
    Recursive,

    /// Watch only the provided directory
    NonRecursive,
}

impl RecursiveMode {
    fn is_recursive(&self) -> bool  {
        match *self {
            RecursiveMode::Recursive => true,
            RecursiveMode::NonRecursive => false,
        }
    }
}

/// Type that can deliver file activity notifications
///
/// Watcher is implemented per platform using the best implementation available on that platform. In
/// addition to such event driven implementations, a polling implementation is also provided that
/// should work on any platform.
pub trait Watcher: Sized {
    /// Create a new Watcher.
    ///
    /// Events will be sent using the provided `tx`.
    fn new(tx: Sender<Event>) -> Result<Self>;

    /// Begin watching a new path.
    ///
    /// If the `path` is a directory, `recursive_mode` will be evaluated.
    /// If `recursive_mode` is `RecursiveMode::Recursive` events will be delivered for all files in that tree.
    /// Otherwise only the directory and it's immediate children will be watched.
    ///
    /// If the `path` is a file, `recursive_mode` will be ignored and events will be delivered only for the file.
    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()>;

    /// Stop watching a path.
    ///
    /// # Errors
    ///
    /// Returns an error in the case that `path` has not been watched or if removing the watch fails.
    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()>;
}

/// The recommended `Watcher` implementation for the current platform
#[cfg(target_os = "linux")]
pub type RecommendedWatcher = INotifyWatcher;
/// The recommended `Watcher` implementation for the current platform
#[cfg(target_os = "macos")]
pub type RecommendedWatcher = FsEventWatcher;
/// The recommended `Watcher` implementation for the current platform
#[cfg(target_os = "windows")]
pub type RecommendedWatcher = ReadDirectoryChangesWatcher;
/// The recommended `Watcher` implementation for the current platform
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub type RecommendedWatcher = PollWatcher;

/// Convenience method for creating the `RecommendedWatcher` for the current platform.
///
/// Events will be sent using the provided `tx`.
pub fn new(tx: Sender<Event>) -> Result<RecommendedWatcher> {
    Watcher::new(tx)
}


#[test]
fn display_formatted_errors() {
    let expected = "Some error";

    assert_eq!(expected,
               format!("{}", Error::Generic(String::from(expected))));

    assert_eq!(expected,
               format!("{}",
                       Error::Io(io::Error::new(io::ErrorKind::Other, expected))));
}
