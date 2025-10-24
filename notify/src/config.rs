//! Configuration types

use std::{path::PathBuf, time::Duration};

/// Indicates whether only the provided directory or its sub-directories as well should be watched
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum RecursiveMode {
    /// Watch all sub-directories as well, including directories created after installing the watch
    Recursive,

    /// Watch only the provided directory
    NonRecursive,
}

impl RecursiveMode {
    pub(crate) fn is_recursive(&self) -> bool {
        match *self {
            RecursiveMode::Recursive => true,
            RecursiveMode::NonRecursive => false,
        }
    }
}

/// Watcher Backend configuration
///
/// This contains multiple settings that may relate to only one specific backend,
/// such as to correctly configure each backend regardless of what is selected during runtime.
///
/// ```rust
/// # use std::time::Duration;
/// # use notify::Config;
/// let config = Config::default()
///     .with_poll_interval(Duration::from_secs(2))
///     .with_compare_contents(true);
/// ```
///
/// Some options can be changed during runtime, others have to be set when creating the watcher backend.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct Config {
    /// See [Config::with_poll_interval]
    poll_interval: Option<Duration>,

    /// See [Config::with_compare_contents]
    compare_contents: bool,

    follow_symlinks: bool,
}

impl Config {
    /// For the [`PollWatcher`](crate::PollWatcher) backend.
    ///
    /// Interval between each re-scan attempt. This can be extremely expensive for large
    /// file trees so it is recommended to measure and tune accordingly.
    ///
    /// The default poll frequency is 30 seconds.
    ///
    /// This will enable automatic polling, overwriting [`with_manual_polling()`](Config::with_manual_polling).
    pub fn with_poll_interval(mut self, dur: Duration) -> Self {
        // TODO: v7.0 break signature to option
        self.poll_interval = Some(dur);
        self
    }

    /// Returns current setting
    pub fn poll_interval(&self) -> Option<Duration> {
        // Changed Signature to Option
        self.poll_interval
    }

    /// For the [`PollWatcher`](crate::PollWatcher) backend.
    ///
    /// Disable automatic polling. Requires calling [`crate::PollWatcher::poll()`] manually.
    ///
    /// This will disable automatic polling, overwriting [`with_poll_interval()`](Config::with_poll_interval).
    pub fn with_manual_polling(mut self) -> Self {
        self.poll_interval = None;
        self
    }

    /// For the [`PollWatcher`](crate::PollWatcher) backend.
    ///
    /// Optional feature that will evaluate the contents of changed files to determine if
    /// they have indeed changed using a fast hashing algorithm.  This is especially important
    /// for pseudo filesystems like those on Linux under /sys and /proc which are not obligated
    /// to respect any other filesystem norms such as modification timestamps, file sizes, etc.
    /// By enabling this feature, performance will be significantly impacted as all files will
    /// need to be read and hashed at each `poll_interval`.
    ///
    /// This can't be changed during runtime. Off by default.
    pub fn with_compare_contents(mut self, compare_contents: bool) -> Self {
        self.compare_contents = compare_contents;
        self
    }

    /// Returns current setting
    pub fn compare_contents(&self) -> bool {
        self.compare_contents
    }

    /// For the [INotifyWatcher](crate::INotifyWatcher), [KqueueWatcher](crate::KqueueWatcher),
    /// and [PollWatcher](crate::PollWatcher).
    ///
    /// Determine if symbolic links should be followed when recursively watching a directory.
    ///
    /// This can't be changed during runtime. On by default.
    pub fn with_follow_symlinks(mut self, follow_symlinks: bool) -> Self {
        self.follow_symlinks = follow_symlinks;
        self
    }

    /// Returns current setting
    pub fn follow_symlinks(&self) -> bool {
        self.follow_symlinks
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Some(Duration::from_secs(30)),
            compare_contents: false,
            follow_symlinks: true,
        }
    }
}

/// Single watch backend configuration
///
/// This contains some settings that may relate to only one specific backend,
/// such as to correctly configure each backend regardless of what is selected during runtime.
#[derive(Debug)]
pub struct WatchPathConfig {
    recursive_mode: RecursiveMode,
}

impl WatchPathConfig {
    /// Creates new instance with provided [`RecursiveMode`]
    pub fn new(recursive_mode: RecursiveMode) -> Self {
        Self { recursive_mode }
    }

    /// Set [`RecursiveMode`] for the watch
    pub fn with_recursive_mode(mut self, recursive_mode: RecursiveMode) -> Self {
        self.recursive_mode = recursive_mode;
        self
    }

    /// Returns current setting
    pub fn recursive_mode(&self) -> RecursiveMode {
        self.recursive_mode
    }
}

/// An operation to apply to a watcher
///
/// See [`Watcher::update_paths`] for more information
#[derive(Debug)]
pub enum PathOp {
    /// Path should be watched
    Watch(PathBuf, WatchPathConfig),

    /// Path should be unwatched
    Unwatch(PathBuf),
}

impl PathOp {
    /// Watch the path with [`RecursiveMode::Recursive`]
    pub fn watch_recursive<P: Into<PathBuf>>(path: P) -> Self {
        Self::Watch(path.into(), WatchPathConfig::new(RecursiveMode::Recursive))
    }

    /// Watch the path with [`RecursiveMode::NonRecursive`]
    pub fn watch_non_recursive<P: Into<PathBuf>>(path: P) -> Self {
        Self::Watch(
            path.into(),
            WatchPathConfig::new(RecursiveMode::NonRecursive),
        )
    }

    /// Unwatch the path
    pub fn unwatch<P: Into<PathBuf>>(path: P) -> Self {
        Self::Unwatch(path.into())
    }
}
