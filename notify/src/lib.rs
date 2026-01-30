//! Cross-platform file system notification library
//!
//! # Installation
//!
//! ```toml
//! [dependencies]
//! notify = "8.1.0"
//! ```
//!
//! If you want debounced events (or don't need them in-order), see [notify-debouncer-mini](https://docs.rs/notify-debouncer-mini/latest/notify_debouncer_mini/)
//! or [notify-debouncer-full](https://docs.rs/notify-debouncer-full/latest/notify_debouncer_full/).
//!
//! ## Features
//!
//! List of compilation features, see below for details
//!
//! - `serde` for serialization of events
//! - `macos_fsevent` enabled by default, for fsevent backend on macos
//! - `macos_kqueue` for kqueue backend on macos
//! - `serialization-compat-6` restores the serialization behavior of notify 6, off by default
//!
//! ### Serde
//!
//! Events are serializable via [serde](https://serde.rs) if the `serde` feature is enabled:
//!
//! ```toml
//! notify = { version = "8.1.0", features = ["serde"] }
//! ```
//!
//! # Known Problems
//!
//! ### Network filesystems
//!
//! Network mounted filesystems like NFS may not emit any events for notify to listen to.
//! This applies especially to WSL programs watching windows paths ([issue #254](https://github.com/notify-rs/notify/issues/254)).
//!
//! A workaround is the [`PollWatcher`] backend.
//!
//! ### Docker with Linux on macOS M1
//!
//! Docker on macOS M1 [throws](https://github.com/notify-rs/notify/issues/423) `Function not implemented (os error 38)`.
//! You have to manually use the [`PollWatcher`], as the native backend isn't available inside the emulation.
//!
//! ### macOS, FSEvents and unowned files
//!
//! Due to the inner security model of FSEvents (see [FileSystemEventSecurity](https://developer.apple.com/library/mac/documentation/Darwin/Conceptual/FSEvents_ProgGuide/FileSystemEventSecurity/FileSystemEventSecurity.html)),
//! some events cannot be observed easily when trying to follow files that do not
//! belong to you. In this case, reverting to the pollwatcher can fix the issue,
//! with a slight performance cost.
//!
//! ### Editor Behaviour
//!
//! If you rely on precise events (Write/Delete/Create..), you will notice that the actual events
//! can differ a lot between file editors. Some truncate the file on save, some create a new one and replace the old one.
//! See also [this](https://github.com/notify-rs/notify/issues/247) and [this](https://github.com/notify-rs/notify/issues/113#issuecomment-281836995) issues for example.
//!
//! ### Parent folder deletion
//!
//! If you want to receive an event for a deletion of folder `b` for the path `/a/b/..`, you will have to watch its parent `/a`.
//! See [here](https://github.com/notify-rs/notify/issues/403) for more details.
//!
//! ### Pseudo Filesystems like /proc, /sys
//!
//! Some filesystems like `/proc` and `/sys` on *nix do not emit change events or use correct file change dates.
//! To circumvent that problem you can use the [`PollWatcher`] with the `compare_contents` option.
//!
//! ### Linux: Bad File Descriptor / No space left on device
//!
//! This may be the case of running into the max-files watched limits of your user or system.
//! (Files also includes folders.) Note that for recursive watched folders each file and folder inside counts towards the limit.
//!
//! You may increase this limit in linux via
//! ```sh
//! sudo sysctl fs.inotify.max_user_instances=8192 # example number
//! sudo sysctl fs.inotify.max_user_watches=524288 # example number
//! sudo sysctl -p
//! ```
//!
//! Note that the [`PollWatcher`] is not restricted by this limitation, so it may be an alternative if your users can't increase the limit.
//!
//! ### Watching large directories
//!
//! When watching a very large amount of files, notify may fail to receive all events.
//! For example the linux backend is documented to not be a 100% reliable source. See also issue [#412](https://github.com/notify-rs/notify/issues/412).
//!
//! # Examples
//!
//! For more examples visit the [examples folder](https://github.com/notify-rs/notify/tree/main/examples) in the repository.
//!
//! ```rust
//! use notify::{Event, RecursiveMode, Result, Watcher};
//! use std::{path::Path, sync::mpsc};
//!
//! fn main() -> Result<()> {
//!     let (tx, rx) = mpsc::channel::<Result<Event>>();
//!
//!     // Use recommended_watcher() to automatically select the best implementation
//!     // for your platform. The `EventHandler` passed to this constructor can be a
//!     // closure, a `std::sync::mpsc::Sender`, a `crossbeam_channel::Sender`, or
//!     // another type the trait is implemented for.
//!     let mut watcher = notify::recommended_watcher(tx)?;
//!
//!     // Add a path to be watched. All files and directories at that path and
//!     // below will be monitored for changes.
//! #     #[cfg(not(any(
//! #     target_os = "freebsd",
//! #     target_os = "openbsd",
//! #     target_os = "dragonfly",
//! #     target_os = "netbsd")))]
//! #     { // "." doesn't exist on BSD for some reason in CI
//!     watcher.watch(Path::new("."), RecursiveMode::Recursive)?;
//! #     }
//! #     #[cfg(any())]
//! #     { // don't run this in doctests, it blocks forever
//!     // Block forever, printing out events as they come in
//!     for res in rx {
//!         match res {
//!             Ok(event) => println!("event: {:?}", event),
//!             Err(e) => println!("watch error: {:?}", e),
//!         }
//!     }
//! #     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## With different configurations
//!
//! It is possible to create several watchers with different configurations or implementations that
//! all call the same event function. This can accommodate advanced behaviour or work around limits.
//!
//! ```rust
//! # use notify::{RecursiveMode, Result, Watcher};
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
//!       // we will just use the same watcher kind again here
//!       let mut watcher2 = notify::recommended_watcher(event_fn)?;
//! #     #[cfg(not(any(
//! #     target_os = "freebsd",
//! #     target_os = "openbsd",
//! #     target_os = "dragonfly",
//! #     target_os = "netbsd")))]
//! #     { // "." doesn't exist on BSD for some reason in CI
//! #     watcher1.watch(Path::new("."), RecursiveMode::Recursive)?;
//! #     watcher2.watch(Path::new("."), RecursiveMode::Recursive)?;
//! #     }
//!       // dropping the watcher1/2 here (no loop etc) will end the program
//! #
//! #     Ok(())
//! # }
//! ```

#![deny(missing_docs)]

pub use config::{Config, PathOp, RecursiveMode, WatchPathConfig};
pub use error::{Error, ErrorKind, Result, UpdatePathsError};
pub use notify_types::event::{self, Event, EventKind, EventKindMask};
use std::path::Path;

pub(crate) type StdResult<T, E> = std::result::Result<T, E>;
pub(crate) type Receiver<T> = std::sync::mpsc::Receiver<T>;
pub(crate) type Sender<T> = std::sync::mpsc::Sender<T>;
#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
pub(crate) type BoundSender<T> = std::sync::mpsc::SyncSender<T>;

#[inline]
pub(crate) fn unbounded<T>() -> (Sender<T>, Receiver<T>) {
    std::sync::mpsc::channel()
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
#[inline]
pub(crate) fn bounded<T>(cap: usize) -> (BoundSender<T>, Receiver<T>) {
    std::sync::mpsc::sync_channel(cap)
}

#[cfg(all(target_os = "macos", not(feature = "macos_kqueue")))]
pub use crate::fsevent::FsEventWatcher;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub use crate::inotify::INotifyWatcher;
#[cfg(any(
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "ios",
    all(target_os = "macos", feature = "macos_kqueue")
))]
pub use crate::kqueue::KqueueWatcher;
pub use null::NullWatcher;
pub use poll::PollWatcher;
#[cfg(target_os = "windows")]
pub use windows::ReadDirectoryChangesWatcher;

#[cfg(all(target_os = "macos", not(feature = "macos_kqueue")))]
pub mod fsevent;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod inotify;
#[cfg(any(
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "ios",
    all(target_os = "macos", feature = "macos_kqueue")
))]
pub mod kqueue;
#[cfg(target_os = "windows")]
pub mod windows;

pub mod null;
pub mod poll;

mod config;
mod error;

#[cfg(test)]
pub(crate) mod test;

/// The set of requirements for watcher event handling functions.
///
/// # Example implementation
///
/// ```no_run
/// use notify::{Event, Result, EventHandler};
///
/// /// Prints received events
/// struct EventPrinter;
///
/// impl EventHandler for EventPrinter {
///     fn handle_event(&mut self, event: Result<Event>) {
///         if let Ok(event) = event {
///             println!("Event: {:?}", event);
///         }
///     }
/// }
/// ```
pub trait EventHandler: Send + 'static {
    /// Handles an event.
    fn handle_event(&mut self, event: Result<Event>);
}

impl<F> EventHandler for F
where
    F: FnMut(Result<Event>) + Send + 'static,
{
    fn handle_event(&mut self, event: Result<Event>) {
        (self)(event);
    }
}

#[cfg(feature = "crossbeam-channel")]
impl EventHandler for crossbeam_channel::Sender<Result<Event>> {
    fn handle_event(&mut self, event: Result<Event>) {
        let _ = self.send(event);
    }
}

#[cfg(feature = "flume")]
impl EventHandler for flume::Sender<Result<Event>> {
    fn handle_event(&mut self, event: Result<Event>) {
        let _ = self.send(event);
    }
}

#[cfg(feature = "futures")]
impl EventHandler for futures::channel::mpsc::UnboundedSender<Result<Event>> {
    fn handle_event(&mut self, event: Result<Event>) {
        let _ = self.unbounded_send(event);
    }
}

#[cfg(feature = "tokio")]
impl EventHandler for tokio::sync::mpsc::UnboundedSender<Result<Event>> {
    fn handle_event(&mut self, event: Result<Event>) {
        let _ = self.send(event);
    }
}

impl EventHandler for std::sync::mpsc::Sender<Result<Event>> {
    fn handle_event(&mut self, event: Result<Event>) {
        let _ = self.send(event);
    }
}

/// Watcher kind enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WatcherKind {
    /// inotify backend (linux)
    Inotify,
    /// FS-Event backend (mac)
    Fsevent,
    /// KQueue backend (bsd,optionally mac)
    Kqueue,
    /// Polling based backend (fallback)
    PollWatcher,
    /// Windows backend
    ReadDirectoryChangesWatcher,
    /// Fake watcher for testing
    NullWatcher,
}

/// Type that can deliver file activity notifications
///
/// `Watcher` is implemented per platform using the best implementation available on that platform.
/// In addition to such event driven implementations, a polling implementation is also provided
/// that should work on any platform.
pub trait Watcher {
    /// Create a new watcher with an initial Config.
    fn new<F: EventHandler>(event_handler: F, config: config::Config) -> Result<Self>
    where
        Self: Sized;
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

    /// Add/remove paths to watch in batch.
    ///
    /// For some [`Watcher`] implementations this method provides better performance than multiple
    /// calls to [`Watcher::watch`] and [`Watcher::unwatch`] if you want to add/remove many paths at once.
    ///
    /// # Examples
    ///
    /// ```
    /// # use notify::{Watcher, RecursiveMode, PathOp};
    /// # use std::path::{Path, PathBuf};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let many_paths_to_add: Vec<PathBuf> = vec![];
    /// # let many_paths_to_remove: Vec<PathBuf> = vec![];
    /// let mut watcher = notify::NullWatcher;
    /// let mut batch = Vec::new();
    ///
    /// for path in many_paths_to_add {
    ///     batch.push(PathOp::watch_recursive(path));
    /// }
    ///
    /// for path in many_paths_to_remove {
    ///     batch.push(PathOp::unwatch(path));
    /// }
    ///
    /// // real work is done there
    /// watcher.update_paths(batch)?;
    /// # Ok(())
    /// # }
    /// ```
    fn update_paths(&mut self, ops: Vec<PathOp>) -> StdResult<(), UpdatePathsError> {
        update_paths(ops, |op| match op {
            PathOp::Watch(path, config) => self.watch(&path, config.recursive_mode()),
            PathOp::Unwatch(path) => self.unwatch(&path),
        })
    }

    /// Configure the watcher at runtime.
    ///
    /// See the [`Config`](config/struct.Config.html) struct for all configuration options.
    ///
    /// # Returns
    ///
    /// - `Ok(true)` on success.
    /// - `Ok(false)` if the watcher does not support or implement the option.
    /// - `Err(notify::Error)` on failure.
    fn configure(&mut self, _option: Config) -> Result<bool> {
        Ok(false)
    }

    /// Returns the watcher kind, allowing to perform backend-specific tasks
    fn kind() -> WatcherKind
    where
        Self: Sized;
}

/// The recommended [`Watcher`] implementation for the current platform
#[cfg(any(target_os = "linux", target_os = "android"))]
pub type RecommendedWatcher = INotifyWatcher;
/// The recommended [`Watcher`] implementation for the current platform
#[cfg(all(target_os = "macos", not(feature = "macos_kqueue")))]
pub type RecommendedWatcher = FsEventWatcher;
/// The recommended [`Watcher`] implementation for the current platform
#[cfg(target_os = "windows")]
pub type RecommendedWatcher = ReadDirectoryChangesWatcher;
/// The recommended [`Watcher`] implementation for the current platform
#[cfg(any(
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "ios",
    all(target_os = "macos", feature = "macos_kqueue")
))]
pub type RecommendedWatcher = KqueueWatcher;
/// The recommended [`Watcher`] implementation for the current platform
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "windows",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "ios"
)))]
pub type RecommendedWatcher = PollWatcher;

/// Convenience method for creating the [`RecommendedWatcher`] for the current platform.
pub fn recommended_watcher<F>(event_handler: F) -> Result<RecommendedWatcher>
where
    F: EventHandler,
{
    // All recommended watchers currently implement `new`, so just call that.
    RecommendedWatcher::new(event_handler, Config::default())
}

pub(crate) fn update_paths<F>(ops: Vec<PathOp>, mut apply: F) -> StdResult<(), UpdatePathsError>
where
    F: FnMut(PathOp) -> Result<()>,
{
    let mut iter = ops.into_iter();
    while let Some(op) = iter.next() {
        if let Err(source) = apply(op) {
            return Err(UpdatePathsError {
                source,
                remaining: iter.collect(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs, iter,
        sync::mpsc,
        time::{Duration, Instant},
    };

    use tempfile::tempdir;

    use super::{
        Config, Error, ErrorKind, Event, NullWatcher, PollWatcher, RecommendedWatcher,
        RecursiveMode, Result, Watcher, WatcherKind, PathOp, WatchPathConfig, StdResult
    };
    use crate::test::*;

    #[test]
    fn test_object_safe() {
        let _watcher: &dyn Watcher = &NullWatcher;
    }

    #[test]
    fn test_debug_impl() {
        macro_rules! assert_debug_impl {
            ($t:ty) => {{
                #[allow(dead_code)]
                trait NeedsDebug: std::fmt::Debug {}
                impl NeedsDebug for $t {}
            }};
        }

        assert_debug_impl!(Config);
        assert_debug_impl!(Error);
        assert_debug_impl!(ErrorKind);
        assert_debug_impl!(NullWatcher);
        assert_debug_impl!(PollWatcher);
        assert_debug_impl!(RecommendedWatcher);
        assert_debug_impl!(RecursiveMode);
        assert_debug_impl!(WatcherKind);
    }

    fn iter_with_timeout(rx: &mpsc::Receiver<Result<Event>>) -> impl Iterator<Item = Event> + '_ {
        // wait for up to 10 seconds for the events
        let deadline = Instant::now() + Duration::from_secs(10);
        iter::from_fn(move || {
            if Instant::now() >= deadline {
                return None;
            }
            Some(
                rx.recv_timeout(deadline - Instant::now())
                    .expect("did not receive expected event")
                    .expect("received an error"),
            )
        })
    }

    #[test]
    fn integration() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;

        // set up the watcher
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
        watcher.watch(dir.path(), RecursiveMode::Recursive)?;

        // create a new file
        let file_path = dir.path().join("file.txt");
        fs::write(&file_path, b"Lorem ipsum")?;

        println!("waiting for event at {}", file_path.display());

        // wait for the create event, ignore all other events
        for event in iter_with_timeout(&rx) {
            if event.paths == vec![file_path.clone()]
                || event.paths == vec![file_path.canonicalize()?]
            {
                return Ok(());
            }

            println!("unexpected event: {event:?}");
        }

        panic!("did not receive expected event");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_windows_trash_dir() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let child_dir = dir.path().join("child");
        fs::create_dir(&child_dir)?;

        let mut watcher = recommended_watcher(|_| {
            // Do something with the event
        })?;
        watcher.watch(&child_dir, RecursiveMode::NonRecursive)?;

        trash::delete(&child_dir)?;

        watcher.watch(dir.path(), RecursiveMode::NonRecursive)?;

        Ok(())
    }

    #[test]
    fn test_update_paths() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;

        let dir_a = dir.path().join("a");
        let dir_b = dir.path().join("b");

        fs::create_dir(&dir_a)?;
        fs::create_dir(&dir_b)?;

        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, Config::default())?;

        // start watching a and b
        watcher.update_paths(vec![
            PathOp::Watch(
                dir_a.clone(),
                WatchPathConfig::new(RecursiveMode::Recursive),
            ),
            PathOp::Watch(
                dir_b.clone(),
                WatchPathConfig::new(RecursiveMode::Recursive),
            ),
        ])?;

        // create file1 in both a and b
        let a_file1 = dir_a.join("file1");
        let b_file1 = dir_b.join("file1");
        fs::write(&a_file1, b"Lorem ipsum")?;
        fs::write(&b_file1, b"Lorem ipsum")?;

        // wait for create events of a/file1 and b/file1
        let mut a_file1_encountered: bool = false;
        let mut b_file1_encountered: bool = false;
        for event in iter_with_timeout(&rx) {
            for path in event.paths {
                a_file1_encountered =
                    a_file1_encountered || (path == a_file1 || path == a_file1.canonicalize()?);
                b_file1_encountered =
                    b_file1_encountered || (path == b_file1 || path == b_file1.canonicalize()?);
            }
            if a_file1_encountered && b_file1_encountered {
                break;
            }
        }
        assert!(a_file1_encountered, "Did not receive event of {a_file1:?}");
        assert!(b_file1_encountered, "Did not receive event of {b_file1:?}");

        // stop watching a
        watcher.update_paths(vec![PathOp::unwatch(&dir_a)])?;

        // create file2 in both a and b
        let a_file2 = dir_a.join("file2");
        let b_file2 = dir_b.join("file2");
        fs::write(&a_file2, b"Lorem ipsum")?;
        fs::write(&b_file2, b"Lorem ipsum")?;

        // wait for the create event of b/file2 only
        for event in iter_with_timeout(&rx) {
            for path in event.paths {
                assert!(
                    path != a_file2 || path != a_file2.canonicalize()?,
                    "Event of {a_file2:?} should not be received"
                );
                if path == b_file2 || path == b_file2.canonicalize()? {
                    return Ok(());
                }
            }
        }
        panic!("Did not receive the event of {b_file2:?}");
    }

    #[test]
    fn update_paths_in_a_loop_with_errors() -> StdResult<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let existing_dir_1 = dir.path().join("existing_dir_1");
        let not_existent_file = dir.path().join("not_existent_file");
        let existing_dir_2 = dir.path().join("existing_dir_2");

        fs::create_dir(&existing_dir_1)?;
        fs::create_dir(&existing_dir_2)?;

        let mut paths_to_add = vec![
            PathOp::watch_recursive(existing_dir_1.clone()),
            PathOp::watch_recursive(not_existent_file.clone()),
            PathOp::watch_recursive(existing_dir_2.clone()),
        ];

        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, Config::default())?;

        while !paths_to_add.is_empty() {
            if let Err(e) = watcher.update_paths(std::mem::take(&mut paths_to_add)) {
                paths_to_add = e.remaining;
            }
        }

        fs::write(existing_dir_1.join("1"), "")?;
        fs::write(&not_existent_file, "")?;
        let waiting_path = existing_dir_2.join("1");
        fs::write(&waiting_path, "")?;

        for event in iter_with_timeout(&rx) {
            let path = event
                .paths
                .first()
                .unwrap_or_else(|| panic!("event must have a path: {event:?}"));
            assert!(
                path != &not_existent_file,
                "unexpected {:?} event",
                not_existent_file
            );
            if path == &waiting_path || path == &waiting_path.canonicalize()? {
                return Ok(());
            }
        }

        panic!("Did not receive the event of {waiting_path:?}");
    }

    #[test]
    fn create_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = recommended_channel();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        rx.wait_unordered([expected(path).create()]);
    }

    #[test]
    fn create_dir() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = recommended_channel();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create");

        rx.wait_unordered([expected(path).create()]);
    }

    #[test]
    fn modify_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = recommended_channel();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        std::fs::write(&path, b"123").expect("write");

        rx.wait_unordered([expected(path).modify()]);
    }

    #[test]
    fn remove_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = recommended_channel();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        std::fs::remove_file(&path).expect("remove");

        rx.wait_unordered([expected(path).remove()]);
    }

    #[cfg(feature = "futures")]
    #[tokio::test]
    async fn futures_unbounded_sender_as_handler() {
        use crate::recommended_watcher;
        use futures::StreamExt;

        let tmpdir = testdir();
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let mut watcher = recommended_watcher(tx).unwrap();
        watcher
            .watch(tmpdir.path(), RecursiveMode::NonRecursive)
            .unwrap();

        std::fs::create_dir(tmpdir.path().join("1")).unwrap();

        tokio::time::timeout(Duration::from_secs(5), rx.next())
            .await
            .expect("timeout")
            .expect("No event")
            .expect("Error");
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn tokio_unbounded_sender_as_handler() {
        use crate::recommended_watcher;

        let tmpdir = testdir();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher = recommended_watcher(tx).unwrap();
        watcher
            .watch(tmpdir.path(), RecursiveMode::NonRecursive)
            .unwrap();

        std::fs::create_dir(tmpdir.path().join("1")).unwrap();

        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("No event")
            .expect("Error");
    }
}
