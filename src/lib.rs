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
//! ## With precise events
//!
//! By default, Notify emits non-descript events containing only the affected path and some
//! metadata. To get richer details about _what_ the events are about, you need to enable
//! [`Config::PreciseEvents`](config/enum.Config.html#variant.PreciseEvents). The full event
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
//! #     let (tx, rx) = unbounded();
//! #
//!       let mut watcher: RecommendedWatcher = Watcher::new_immediate(tx)?;
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
//!
//! ## With different configurations
//!
//! It is possible to create several watchers with different configurations or implementations that
//! all send to the same channel. This can accommodate advanced behaviour or work around limits.
//!
//! ```
//! # extern crate crossbeam_channel;
//! # extern crate notify;
//! #
//! # use crossbeam_channel::unbounded;
//! # use notify::{Watcher, RecommendedWatcher, RecursiveMode, Result};
//! #
//! # fn main() -> Result<()> {
//! #     let (tx, rx) = unbounded();
//! #
//!       let mut watcher1: RecommendedWatcher = Watcher::new_immediate(tx.clone())?;
//!       let mut watcher2: RecommendedWatcher = Watcher::new_immediate(tx)?;
//! #
//! #     watcher1.watch(".", RecursiveMode::Recursive)?;
//! #     watcher2.watch(".", RecursiveMode::Recursive)?;
//! #
//!       loop {
//! #         break;
//!           match rx.recv() {
//!              Ok(event) => println!("event: {:?}", event),
//!              Err(e) => println!("watch error: {:?}", e),
//!           }
//!       }
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

pub use self::config::{Config, RecursiveMode};
pub use self::error::{Error, ErrorKind, Result};
pub use self::event::{Event, EventKind};
pub use self::raw_event::{op, Op, RawEvent};
use crossbeam_channel::Sender;
use std::convert::AsRef;
use std::path::Path;
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

mod config;
mod debounce;
mod error;
mod raw_event;

/// Type that can deliver file activity notifications
///
/// Watcher is implemented per platform using the best implementation available on that platform.
/// In addition to such event driven implementations, a polling implementation is also provided
/// that should work on any platform.
pub trait Watcher: Sized {
    /// Create a new watcher in _immediate_ mode.
    ///
    /// Events will be sent using the provided `tx` immediately after they occur.
    fn new_immediate(tx: Sender<RawEvent>) -> Result<Self>;

    /// Create a new _debounced_ watcher with a `delay`.
    ///
    /// Events won't be sent immediately; every iteration they will be collected, deduplicated, and
    /// emitted only after the specified delay.
    ///
    /// # Advantages
    ///
    /// This has the advantage that a lot of logic can be offloaded to Notify.
    ///
    /// For example you won't have to handle `Modify(Name(From|To))` events yourself by piecing the
    /// two rename events together. Instead you will just receive a `Modify(Name(Both))` with two
    /// paths `from` and `to` in that order.
    ///
    /// Notify will also detect the beginning and the end of write operations. As soon as something
    /// is written to a file, a `Modify` notice is emitted. If no new event arrived until after the
    /// specified `delay`, a `Modify` event is emitted.
    ///
    /// A practical example is "safe-saving", where a temporary file is created and written to, then
    /// only when everything has been written is it renamed to overwrite the file that was meant to
    /// be saved. Instead of receiving a `Create` event for the temporary file, `Modify(Data)`
    /// events to that file, and a `Modify(Name)` event from the temporary file to the file being
    /// saved, you will just receive a single `Modify(Data)` event.
    ///
    /// If you use a delay of more than 30 seconds, you can avoid receiving repetitions of previous
    /// events on macOS.
    ///
    /// # Disadvantages
    ///
    /// Your application might not feel as responsive.
    ///
    /// If a file is saved very slowly, you might receive a `Modify` event even though the file is
    /// still being written to.
    fn new(tx: Sender<Result<Event>>, delay: Duration) -> Result<Self>;

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
    /// See the [`Config`](config/enum.Config.html) enum for all configuration options.
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

/// Convenience method for creating the `RecommendedWatcher` for the current platform in
/// _immediate_ mode.
///
/// See [`Watcher::new_immediate`](trait.Watcher.html#tymethod.new_immediate).
pub fn immediate_watcher(tx: Sender<RawEvent>) -> Result<RecommendedWatcher> {
    Watcher::new_immediate(tx)
}

/// Convenience method for creating the `RecommendedWatcher` for the current
/// platform in default (debounced) mode.
///
/// See [`Watcher::new`](trait.Watcher.html#tymethod.new).
pub fn watcher(tx: Sender<Result<Event>>, delay: Duration) -> Result<RecommendedWatcher> {
    Watcher::new(tx, delay)
}
