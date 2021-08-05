//! Cross-platform file system notification library
//!
//! # Installation
//!
//! ```toml
//! [dependencies]
//! notify = "5.0.0-pre.11"
//! ```
//!
//! ## Serde
//!
//! Events are serialisable via [serde] if the `serde` feature is enabled:
//!
//! ```toml
//! notify = { version = "5.0.0-pre.11", features = ["serde"] }
//! ```
//!
//! [serde]: https://serde.rs
//!
//! # Examples
//!
//! ```
//! # use std::path::Path;
//! use notify::{Watcher, RecommendedWatcher, RecursiveMode, Result};
//!
//! fn main() -> Result<()> {
//!     // Automatically select the best implementation for your platform.
//!     let mut watcher = notify::recommended_watcher(|res| {
//!         match res {
//!            Ok(event) => println!("event: {:?}", event),
//!            Err(e) => println!("watch error: {:?}", e),
//!         }
//!     })?;
//!
//!     // Add a path to be watched. All files and directories at that path and
//!     // below will be monitored for changes.
//!     watcher.watch(Path::new("."), RecursiveMode::Recursive)?;
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
//! classification is described in the [`event`](event/index.html) module documentation.
//!
//! ```
//! # use notify::{Watcher, RecommendedWatcher, RecursiveMode, Result};
//! # use std::path::Path;
//! # use std::time::Duration;
//! # fn main() -> Result<()> {
//! # // Automatically select the best implementation for your platform.
//! # let mut watcher = RecommendedWatcher::new(|res| {
//! #     match res {
//! #        Ok(event) => println!("event: {:?}", event),
//! #        Err(e) => println!("watch error: {:?}", e),
//! #     }
//! # })?;
//!
//! # // Add a path to be watched. All files and directories at that path and
//! # // below will be monitored for changes.
//! # watcher.watch(Path::new("."), RecursiveMode::Recursive)?;
//!
//! use notify::Config;
//! watcher.configure(Config::PreciseEvents(true))?;
//!
//! # Ok(())
//! # }
//!
//! ```
//!
//! ## With different configurations
//!
//! It is possible to create several watchers with different configurations or implementations that
//! all call the same event function. This can accommodate advanced behaviour or work around limits.
//!
//! ```
//! # use notify::{RecommendedWatcher, RecursiveMode, Result, Watcher};
//! # use std::path::Path;
//! #
//! # fn main() -> Result<()> {
//!       fn event_fn(res: Result<notify::Event>) {
//!           match res {
//!              Ok(event) => println!("event: {:?}", event),
//!              Err(e) => println!("watch error: {:?}", e),
//!           }
//!       }
//!
//!       let mut watcher1 = notify::recommended_watcher(event_fn)?;
//!       let mut watcher2 = notify::recommended_watcher(event_fn)?;
//! #     watcher1.watch(Path::new("."), RecursiveMode::Recursive)?;
//! #     watcher2.watch(Path::new("."), RecursiveMode::Recursive)?;
//! #
//! #     Ok(())
//! # }
//! ```

#![deny(missing_docs)]

pub use config::{Config, RecursiveMode};
pub use error::{Error, ErrorKind, Result};
pub use event::{Event, EventKind};
use std::path::Path;

#[cfg(all(target_os = "macos", feature = "macos_fsevent"))]
pub use crate::fsevent::FsEventWatcher;
#[cfg(target_os = "linux")]
pub use crate::inotify::INotifyWatcher;
#[cfg(target_os = "freebsd")]
pub use crate::kqueue::KqueueWatcher;
pub use null::NullWatcher;
pub use poll::PollWatcher;
#[cfg(target_os = "windows")]
pub use windows::ReadDirectoryChangesWatcher;

#[cfg(target_os = "macos")]
pub mod fsevent;
#[cfg(target_os = "linux")]
pub mod inotify;
#[cfg(any(
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "dragonflybsd",
    target_os = "netbsd",
    all(target_os = "macos", feature = "macos_kqueue")
))]
pub mod kqueue;
#[cfg(target_os = "windows")]
pub mod windows;

pub mod event;
pub mod null;
pub mod poll;

mod config;
mod error;

/// The set of requirements for watcher event handling functions.
pub trait EventFn: 'static + FnMut(Result<Event>) + Send {}

impl<F> EventFn for F where F: 'static + FnMut(Result<Event>) + Send {}

/// Type that can deliver file activity notifications
///
/// Watcher is implemented per platform using the best implementation available on that platform.
/// In addition to such event driven implementations, a polling implementation is also provided
/// that should work on any platform.
pub trait Watcher {
    /// Create a new watcher.
    fn new<F: EventFn>(event_fn: F) -> Result<Self> where Self: Sized;
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
    /// [#165]: https://github.com/notify-rs/notify/issues/165
    /// [#166]: https://github.com/notify-rs/notify/issues/166
    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()>;

    /// Stop watching a path.
    ///
    /// # Errors
    ///
    /// Returns an error in the case that `path` has not been watched or if removing the watch
    /// fails.
    fn unwatch(&mut self, path: &Path) -> Result<()>;

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
#[cfg(target_os = "freebsd")]
pub type RecommendedWatcher = KqueueWatcher;
/// The recommended `Watcher` implementation for the current platform
#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "windows",
    target_os = "freebsd"
)))]
pub type RecommendedWatcher = PollWatcher;

/// Convenience method for creating the `RecommendedWatcher` for the current platform in
/// _immediate_ mode.
///
/// See [`Watcher::new_immediate`](trait.Watcher.html#tymethod.new_immediate).
pub fn recommended_watcher<F>(event_fn: F) -> Result<RecommendedWatcher>
where
    F: EventFn,
{
    // All recommended watchers currently implement `new`, so just call that.
    RecommendedWatcher::new(event_fn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_safe() {
        let _watcher: &dyn Watcher = &NullWatcher;
    }
}
