//! Cross-platform file system notification library
//!
//! # Installation
//!
//! ```toml
//! [dependencies]
//! notify = "5.0.0"
//! ```
//!
//! ## Upgrading from 4.0
//!
//! A guide is available on the wiki:
//! https://github.com/passcod/notify/wiki/Upgrading-from-4.x-to-5.0
//!
//! ## Serde
//!
//! Events are serialisable via [serde] if the `serde` feature is enabled:
//!
//! ```toml
//! notify = { version = "5.0.0", features = ["serde"] }
//! ```
//!
//! [serde]: https://serde.rs
//!
//! # Examples
//!
//! ```
//! extern crate crossbeam_channel;
//! extern crate notify;
//!
//! use crossbeam_channel::unbounded;
//! use notify::{RecommendedWatcher, RecursiveMode, Result, Watcher};
//! use std::time::Duration;
//!
//! fn main() -> Result<()> {
//!     // Create a channel to receive the events.
//!     let (tx, rx) = unbounded();
//!
//!     // Automatically select the best implementation for your platform.
//!     let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(2))?;
//!
//!     // Add a path to be watched. All files and directories at that path and
//!     // below will be monitored for changes.
//!     watcher.watch(".", RecursiveMode::Recursive)?;
//!
//!     loop {
//! #       break;
//!         match rx.recv() {
//!            Ok(event) => println!("changed: {:?}", event),
//!            Err(err) => println!("watch error: {:?}", err),
//!         };
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ### With precise events
//!
//! By default, Notify emits non-descript events containing only the affected path and some
//! metadata. To get richer details about _what_ the events are about, you need to enable
//! [`Config::PreciseEvents`](enum.Config.html#variant.PreciseEvents). The full event
//! classification is described in the [`event`](event/index.html`) module documentation.
//!
//! ```
//! # extern crate crossbeam_channel;
//! # extern crate notify;
//! # use crossbeam_channel::unbounded;
//! # use notify::{Watcher, RecursiveMode, RecommendedWatcher, Result, watcher};
//! # use std::time::Duration;
//! #
//! # fn main() -> Result<()> {
//! # let (tx, rx) = unbounded();
//! # let mut watcher: RecommendedWatcher = watcher(tx, Duration::from_secs(10))?;
//! #
//! use notify::Config;
//! watcher.configure(Config::PreciseEvents(true))?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Without debouncing
//!
//! To receive events as they are emitted, without debouncing at all:
//!
//! ```
//! # extern crate crossbeam_channel;
//! # extern crate notify;
//! #
//! # use crossbeam_channel::unbounded;
//! # use notify::{Watcher, RecommendedWatcher, RecursiveMode, Result};
//! #
//! # fn main() -> Result<()> {
//! #     // Create a channel to receive the events.
//! #     let (tx, rx) = unbounded();
//! #
//! #     // Create a watcher object, delivering raw events.
//! #     // The notification back-end is selected based on the platform.
//!       let mut watcher: RecommendedWatcher = Watcher::new_immediate(tx)?;
//! #
//! #     // Add a path to be watched. All files and directories at that path and
//! #     // below will be monitored for changes.
//! #     watcher.watch(".", RecursiveMode::Recursive)?;
//! #
//! #     loop {
//! #         break;
//! #         match rx.recv() {
//! #            Ok(event) => println!("event: {:?}", event),
//! #            Err(e) => println!("watch error: {:?}", e),
//! #         }
//! #     }
//! #
//! #     Ok(())
//! # }
//! ```

#![deny(missing_docs)]

extern crate anymap;
#[macro_use]
extern crate bitflags;
extern crate chashmap;
extern crate crossbeam_channel;
extern crate filetime;
#[cfg(target_os = "macos")]
extern crate fsevent_sys;
extern crate libc;
#[cfg(target_os = "linux")]
extern crate mio;
#[cfg(target_os = "linux")]
extern crate mio_extras;
#[cfg(feature = "serde")]
#[allow(unused_imports)] // for 2015-edition macro_use
#[macro_use]
extern crate serde;
#[cfg(target_os = "windows")]
extern crate winapi;

pub use self::op::Op;
use crossbeam_channel::Sender;
use std::convert::AsRef;
use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::result::Result as StdResult;
use std::time::Duration;

#[cfg(target_os = "macos")]
pub use self::fsevent::FsEventWatcher;
#[cfg(target_os = "linux")]
pub use self::inotify::INotifyWatcher;
pub use self::null::NullWatcher;
pub use self::poll::PollWatcher;
#[cfg(target_os = "windows")]
pub use self::windows::ReadDirectoryChangesWatcher;

#[cfg(target_os = "macos")]
pub mod fsevent;
#[cfg(target_os = "linux")]
pub mod inotify;
#[cfg(target_os = "windows")]
pub mod windows;

pub mod event;
pub mod null;
pub mod poll;

mod debounce;

/// Contains the `Op` type which describes the actions for an event.
///
/// `notify` aims to provide unified behavior across platforms. This however is not always possible
/// due to the underlying technology of the various operating systems. So there are some issues
/// `notify`-API users will have to take care of themselves, depending on their needs.
///
///
/// # Metadata
///
/// __Linux, macOS__
///
/// On Linux and macOS the `METADATA` event is emitted whenever attributes or extended attributes
/// change.
///
/// __Windows__
///
/// On Windows a `WRITE` event is emitted when attributes change. This makes it impossible to
/// distinguish between writes to a file or its meta data.
///
///
/// # Close-Write
///
/// A `CLOSE_WRITE` event is emitted whenever a file that was opened for writing has been closed.
///
/// __This event is only available on Linux__.
///
///
/// # Create
///
/// A `CREATE` event is emitted whenever a new file or directory is created.
///
/// Upon receiving a `Create` event for a directory, it is necessary to scan the newly created
/// directory for contents. The directory can contain files or directories if those contents were
/// created before the directory could be watched, or if the directory was moved into the watched
/// directory.
///
/// # Remove
///
/// ## Remove file or directory within a watched directory
///
/// A `REMOVE` event is emitted whenever a file or directory is removed.
///
/// ## Remove watched file or directory itself
///
/// With the exception of Windows a `REMOVE` event is emitted whenever the watched file or
/// directory itself is removed. The behavior after the remove differs between platforms though.
///
/// __Linux__
///
/// When a watched file or directory is removed, its watch gets destroyed and no new events will be
/// sent.
///
/// __Windows__
///
/// If a watched directory is removed, an empty event is emitted.
///
/// When watching a single file on Windows, the file path will continue to be watched until either
/// the watch is removed by the API user or the parent directory gets removed.
///
/// When watching a directory on Windows, the watch will get destroyed and no new events will be
/// sent.
///
/// __macOS__
///
/// While Linux and Windows monitor "inodes", macOS monitors "paths". So a watch stays active even
/// after the watched file or directory has been removed and it will emit events in case a new file
/// or directory is created in its place.
///
///
/// # Rename
///
/// A `RENAME` event is emitted whenever a file or directory has been renamed or moved to a
/// different directory.
///
/// ## Rename file or directory within a watched directory
///
/// __Linux, Windows__
///
/// A rename with both the source and the destination path inside a watched directory produces two
/// `RENAME` events. The first event contains the source path, the second contains the destination
/// path. Both events share the same cookie.
///
/// A rename that originates inside of a watched directory but ends outside of a watched directory
/// produces a `DELETE` event.
///
/// A rename that originates outside of a watched directory and ends inside of a watched directory
/// produces a `CREATE` event.
///
/// __macOS__
///
/// A `RENAME` event is produced whenever a file or directory is moved. This includes moves within
/// the watched directory as well as moves into or out of the watched directory. It is up to the
/// API user to determine what exactly happened. Usually when a move within a watched directory
/// occurs, the cookie is set for both connected events. This can however fail eg. if a file gets
/// renamed multiple times without a delay (test `fsevents_rename_rename_file_0`). So in some cases
/// rename cannot be caught properly but would be interpreted as a sequence of events where a file
/// or directory is moved out of the watched directory and a different file or directory is moved
/// in.
///
/// ## Rename watched file or directory itself
///
/// With the exception of Windows a `RENAME` event is emitted whenever the watched file or
/// directory itself is renamed. The behavior after the rename differs between platforms though.
/// Depending on the platform either the moved file or directory will continue to be watched or the
/// old path. If the moved file or directory will continue to be watched, the paths of emitted
/// events will still be prefixed with the old path though.
///
/// __Linux__
///
/// Linux will continue to watch the moved file or directory. Events will contain paths prefixed
/// with the old path.
///
/// __Windows__
///
/// Currently there is no event emitted when a watched directory is renamed. But the directory will
/// continue to be watched and events will contain paths prefixed with the old path.
///
/// When renaming a watched file, a `RENAME` event is emitted but the old path will continue to be
/// watched.
///
/// __macOS__
///
/// macOS will continue to watch the (now non-existing) path.
///
/// ## Rename parent directory of watched file or directory
///
/// Currently no event will be emitted when any parent directory of the watched file or directory
/// is renamed. Depending on the platform either the moved file or directory will continue to be
/// watched or the old path. If the moved file or directory will continue to be watched, the paths
/// of emitted events will still be prefixed with the old path though.
///
/// __Linux, Windows__
///
/// Linux and Windows will continue to watch the moved file or directory. Events will contain paths
/// prefixed with the old path.
///
/// __macOS__
///
/// macOS will continue to watch the (now non-existing) path.
///
///
/// # Rescan
///
/// A `RESCAN` event indicates that an error occurred and the watched directories need to be
/// rescanned. This can happen if the internal event queue has overflown and some events were
/// dropped. Or with FSEvents if events were coalesced hierarchically.
///
/// __Windows__
///
/// At the moment `RESCAN` events aren't emitted on Windows.
///
/// __Queue size__
///
/// Linux: `/proc/sys/fs/inotify/max_queued_events`
///
/// Windows: 16384 Bytes. The actual amount of events that fit into the queue depends on the length
/// of the paths.
///
///
/// # Write
///
/// A `WRITE` event is emitted whenever a file has been written to.
///
/// __Windows__
///
/// On Windows a `WRITE` event is emitted when attributes change.
#[allow(missing_docs)]
pub mod op {
    bitflags! {
    /// Holds a set of bit flags representing the actions for the event.
    ///
    /// For a list of possible values, have a look at the [notify::op](index.html) documentation.
    ///
    /// Multiple actions may be delivered in a single event.
        pub struct Op: u32 {
    /// Attributes changed
            const METADATA       = 0b0000001;
    /// Created
            const CREATE      = 0b0000010;
    /// Removed
            const REMOVE      = 0b0000100;
    /// Renamed
            const RENAME      = 0b0001000;
    /// Written
            const WRITE       = 0b0010000;
    /// File opened for writing was closed
            const CLOSE_WRITE = 0b0100000;
    /// Directories need to be rescanned
            const RESCAN      = 0b1000000;
        }
    }

    pub const METADATA: Op = Op::METADATA;
    pub const CREATE: Op = Op::CREATE;
    pub const REMOVE: Op = Op::REMOVE;
    pub const RENAME: Op = Op::RENAME;
    pub const WRITE: Op = Op::WRITE;
    pub const CLOSE_WRITE: Op = Op::CLOSE_WRITE;
    pub const RESCAN: Op = Op::RESCAN;
}

#[cfg(test)]
mod op_test {
    #[test]
    fn mixed_bitflags_form() {
        let op = super::op::Op::METADATA | super::op::WRITE;
        assert!(op.contains(super::op::METADATA));
        assert!(op.contains(super::op::Op::WRITE));
    }

    #[test]
    fn new_bitflags_form() {
        let op = super::op::Op::METADATA | super::op::Op::WRITE;
        assert!(op.contains(super::op::Op::WRITE));
        assert!(op.contains(super::op::Op::METADATA));
    }

    #[test]
    fn old_bitflags_form() {
        let op = super::op::METADATA | super::op::WRITE;
        assert!(op.contains(super::op::WRITE));
    }
}

/// Event delivered when action occurs on a watched path in _raw_ mode
#[derive(Debug)]
pub struct RawEvent {
    /// Path where the event originated.
    ///
    /// `path` is always absolute, even if a relative path is used to watch a file or directory.
    ///
    /// On **macOS** the path is always canonicalized.
    ///
    /// Keep in mind that the path may be false if the watched file or directory or any parent
    /// directory is renamed. (See: [notify::op](op/index.html#rename))
    pub path: Option<PathBuf>,

    /// Operation detected on that path.
    ///
    /// When using the `PollWatcher`, `op` may be `Err` if reading meta data for the path fails.
    ///
    /// When using the `INotifyWatcher`, `op` may be `Err` if activity is detected on the file and
    /// there is an error reading from inotify.
    pub op: Result<Op>,

    /// Unique cookie associating related events (for `RENAME` events).
    ///
    /// If two consecutive `RENAME` events share the same cookie, it means that the first event
    /// holds the old path, and the second event holds the new path of the renamed file or
    /// directory.
    ///
    /// For details on handling `RENAME` events with the `FsEventWatcher` have a look at the
    /// [notify::op](op/index.html) documentation.
    pub cookie: Option<u32>,
}

unsafe impl Send for RawEvent {}

#[derive(Debug)]
/// Event delivered when action occurs on a watched path in debounced mode
pub enum DebouncedEvent {
    /// `NoticeWrite` is emitted immediately after the first write event for the path.
    ///
    /// If you are reading from that file, you should probably close it immediately and discard all
    /// data you read from it.
    NoticeWrite(PathBuf),

    /// Event emitted when a file being watched is to be tailed.
    /// `OngoingWrite` is emitted for subsequent write events to the path.
    ///
    /// This event is only emitted when the [`OngoingWrites`] configuration option is enabled for
    /// the watcher. See that variant for more details.
    ///
    /// [`OngoingWrites`]: ./enum.Config.html#variant.OngoingWrites
    OngoingWrite(PathBuf),

    /// `NoticeRemove` is emitted immediately after a remove or rename event for the path.
    ///
    /// The file will continue to exist until its last file handle is closed.
    NoticeRemove(PathBuf),

    /// `Create` is emitted when a file or directory has been created and no events were detected
    /// for the path within the specified time frame.
    ///
    /// `Create` events have a higher priority than `Write` and `Metadata`. These events will not be
    /// emitted if they are detected before the `Create` event has been emitted.
    Create(PathBuf),

    /// `Write` is emitted when a file has been written to and no events were detected for the path
    /// within the specified time frame.
    ///
    /// `Write` events have a higher priority than `Metadata`. `Metadata` will not be emitted if it's
    /// detected before the `Write` event has been emitted.
    ///
    /// Upon receiving a `Create` event for a directory, it is necessary to scan the newly created
    /// directory for contents. The directory can contain files or directories if those contents
    /// were created before the directory could be watched, or if the directory was moved into the
    /// watched directory.
    Write(PathBuf),

    /// `Metadata` is emitted when attributes have been changed and no events were detected for the
    /// path within the specified time frame.
    Metadata(PathBuf),

    /// `Remove` is emitted when a file or directory has been removed and no events were detected
    /// for the path within the specified time frame.
    Remove(PathBuf),

    /// `Rename` is emitted when a file or directory has been moved within a watched directory and
    /// no events were detected for the new path within the specified time frame.
    ///
    /// The first path contains the source, the second path the destination.
    Rename(PathBuf, PathBuf),

    /// `Rescan` is emitted immediately after a problem has been detected that makes it necessary
    /// to re-scan the watched directories.
    Rescan,

    /// `Error` is emitted immediately after a error has been detected.
    ///
    ///  This event may contain a path for which the error was detected.
    Error(Error, Option<PathBuf>),
}

impl PartialEq for DebouncedEvent {
    fn eq(&self, other: &DebouncedEvent) -> bool {
        match (self, other) {
            (&DebouncedEvent::NoticeWrite(ref a), &DebouncedEvent::NoticeWrite(ref b))
            | (&DebouncedEvent::NoticeRemove(ref a), &DebouncedEvent::NoticeRemove(ref b))
            | (&DebouncedEvent::Create(ref a), &DebouncedEvent::Create(ref b))
            | (&DebouncedEvent::Write(ref a), &DebouncedEvent::Write(ref b))
            | (&DebouncedEvent::Metadata(ref a), &DebouncedEvent::Metadata(ref b))
            | (&DebouncedEvent::Remove(ref a), &DebouncedEvent::Remove(ref b)) => a == b,
            (&DebouncedEvent::Rename(ref a1, ref a2), &DebouncedEvent::Rename(ref b1, ref b2)) => {
                (a1 == b1 && a2 == b2)
            }
            (&DebouncedEvent::Rescan, &DebouncedEvent::Rescan) => true,
            _ => false,
        }
    }
}

/// Errors generated from the `notify` crate
#[derive(Debug)]
pub enum Error {
    /// Generic error
    ///
    /// May be used in cases where a platform specific error is mapped to this type, or for opaque
    /// internal errors.
    Generic(String),

    /// I/O errors
    Io(io::Error),

    /// The provided path does not exist
    PathNotFound,

    /// Attempted to remove a watch that does not exist
    WatchNotFound,

    /// An invalid value was passed as runtime configuration
    InvalidConfigValue,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let error = String::from(match *self {
            Error::PathNotFound => "No path was found.",
            Error::WatchNotFound => "No watch was found.",
            Error::InvalidConfigValue => "Invalid configuration value.",
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
            Error::InvalidConfigValue => "Invalid configuration value.",
            Error::Generic(_) => "Generic error",
            Error::Io(_) => "I/O Error",
        }
    }

    fn cause(&self) -> Option<&StdError> {
        match *self {
            Error::Io(ref cause) => Some(cause),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl<T> From<crossbeam_channel::SendError<T>> for Error {
    fn from(err: crossbeam_channel::SendError<T>) -> Self {
        Error::Generic(format!("internal channel disconnect: {:?}", err))
    }
}

impl From<crossbeam_channel::RecvError> for Error {
    fn from(err: crossbeam_channel::RecvError) -> Self {
        Error::Generic(format!("internal channel disconnect: {:?}", err))
    }
}

impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(err: std::sync::PoisonError<T>) -> Self {
        Error::Generic(format!("internal mutex poisoned: {:?}", err))
    }
}

#[cfg(target_os = "linux")]
impl<T> From<mio_extras::channel::SendError<T>> for Error {
    fn from(err: mio_extras::channel::SendError<T>) -> Self {
        Error::Generic(format!("internal channel error: {:?}", err))
    }
}

/// Indicates whether only the provided directory or its sub-directories as well should be watched
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum RecursiveMode {
    /// Watch all sub-directories as well, including directories created after installing the watch
    Recursive,

    /// Watch only the provided directory
    NonRecursive,
}

impl RecursiveMode {
    fn is_recursive(&self) -> bool {
        match *self {
            RecursiveMode::Recursive => true,
            RecursiveMode::NonRecursive => false,
        }
    }
}

/// Type that can deliver file activity notifications
///
/// Watcher is implemented per platform using the best implementation available on that platform.
/// In addition to such event driven implementations, a polling implementation is also provided
/// that should work on any platform.
pub trait Watcher: Sized {
    /// Create a new watcher in _raw_ mode.
    ///
    /// Events will be sent using the provided `tx` immediately after they occurred.
    fn new_immediate(tx: Sender<RawEvent>) -> Result<Self>;

    /// Create a new _debounced_ watcher with a `delay`.
    ///
    /// Events won't be sent immediately but after the specified delay.
    ///
    /// # Advantages
    ///
    /// This has the advantage that a lot of logic can be offloaded to `notify`.
    ///
    /// For example you won't have to handle `RENAME` events yourself by piecing the two parts of
    /// rename events together. Instead you will just receive a `Rename{from: PathBuf, to:
    /// PathBuf}` event.
    ///
    /// Also `notify` will detect the beginning and the end of write operations. As soon as
    /// something is written to a file, a `NoticeWrite` event is emitted. If no new event arrived
    /// until after the specified `delay`, a `Write` event is emitted.
    ///
    /// A practical example would be the safe-saving of a file, where a temporary file is created
    /// and written to, then only when everything has been written to that file is it renamed to
    /// overwrite the file that was meant to be saved. Instead of receiving a `CREATE` event for
    /// the temporary file, `WRITE` events to that file and a `RENAME` event from the temporary
    /// file to the file being saved, you will just receive a single `Write` event.
    ///
    /// If you use a delay of more than 30 seconds, you can avoid receiving repetitions of previous
    /// events on macOS.
    ///
    /// # Disadvantages
    ///
    /// Your application might not feel as responsive.
    ///
    /// If a file is saved very slowly, you might receive a `Write` event even though the file is
    /// still being written to.
    fn new(tx: Sender<DebouncedEvent>, delay: Duration) -> Result<Self>;

    /// Begin watching a new path.
    ///
    /// If the `path` is a directory, `recursive_mode` will be evaluated. If `recursive_mode` is
    /// `RecursiveMode::Recursive` events will be delivered for all files in that tree. Otherwise
    /// only the directory and its immediate children will be watched.
    ///
    /// If the `path` is a file, `recursive_mode` will be ignored and events will be delivered only
    /// for the file.
    ///
    /// On some platforms, if the `path` is renamed or removed while being watched, behaviour may
    /// be unexpected. See discussions in [#165] and [#166]. If less surprising behaviour is wanted
    /// one may non-recursively watch the _parent_ directory as well and manage related events.
    ///
    /// [#165]: https://github.com/passcod/notify/issues/165
    /// [#166]: https://github.com/passcod/notify/issues/166
    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()>;

    /// Stop watching a path.
    ///
    /// # Errors
    ///
    /// Returns an error in the case that `path` has not been watched or if removing the watch
    /// fails.
    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()>;

    /// Configure the watcher at runtime.
    ///
    /// See the [`Config`](./enum.Config.html) enum for all configuration options.
    ///
    /// # Returns
    ///
    /// - `Ok(true)` on success.
    /// - `Ok(false)` if the watcher does not support or implement the option.
    /// - `Err(notify::Error)` on failure.
    fn configure(&mut self, _option: Config) -> Result<bool> {
        Ok(false)
    }
}

/// Runtime configuration items for watchers.
///
/// See the [`Watcher::configure`](./trait.Watcher.html#tymethod.configure) method for usage.
pub enum Config {
    /// Enable or disable emitting precise event classification.
    ///
    /// Applicable to all watchers.
    ///
    /// When enabled, events are emitted with a `kind` set to as much precision about what kind of
    /// event they are as the backend is capable of providing. When disabled (default), events are
    /// instead emitted as `EventKind::Any`.
    PreciseEvents(bool),

    /// Enable or disable emitting `OngoingWrite` events.
    ///
    /// Applicable to debounced watchers only.
    ///
    /// When enabled, raw partial write events that are received after a `NoticeWrite` but before
    /// the end of a debouncing period (and the emission of a `Write` event) are passed through as
    /// [`OngoingWrite`] events. These events are still debounced, but at a lower (configurable)
    /// interval than the debouncing interval.
    ///
    /// To enable, provide `Some(Duration)`. To disable, provide `None`.
    ///
    /// # Errors
    ///
    /// - `InvalidConfigValue` if the interval provided is higher than the debounce interval.
    ///
    /// [`OngoingWrite`]: ./enum.DebouncedEvent.html#variant.OngoingWrite
    OngoingWrites(Option<Duration>),
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

/// Convenience method for creating the `RecommendedWatcher` for the current platform in _raw_ mode.
///
/// See [`Watcher::new_immediate`](trait.Watcher.html#tymethod.new_immediate).
pub fn raw_watcher(tx: Sender<RawEvent>) -> Result<RecommendedWatcher> {
    Watcher::new_immediate(tx)
}

/// Convenience method for creating the `RecommendedWatcher` for the current
/// platform in default (debounced) mode.
///
/// See [`Watcher::new`](trait.Watcher.html#tymethod.new).
pub fn watcher(tx: Sender<DebouncedEvent>, delay: Duration) -> Result<RecommendedWatcher> {
    Watcher::new(tx, delay)
}

#[test]
fn display_formatted_errors() {
    let expected = "Some error";

    assert_eq!(
        expected,
        format!("{}", Error::Generic(String::from(expected)))
    );

    assert_eq!(
        expected,
        format!(
            "{}",
            Error::Io(io::Error::new(io::ErrorKind::Other, expected))
        )
    );
}
