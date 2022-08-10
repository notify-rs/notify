//! Configuration types

use std::time::Duration;

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
    /// See [BackendConfig::with_poll_interval]
    poll_interval: Duration,

    /// See [BackendConfig::with_compare_contents]
    compare_contents: bool,
}

impl Config {
    /// For [crate::PollWatcher]
    /// 
    /// Interval between each rescan attempt. This can be extremely expensive for large
    /// file trees so it is recommended to measure and tune accordingly.
    /// 
    /// The default poll frequency is 30 seconds.
    pub fn with_poll_interval(mut self, dur: Duration) -> Self {
        self.poll_interval = dur;
        self
    }

    /// Returns current setting
    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    /// For [crate::PollWatcher]
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
}

impl Default for Config {
    fn default() -> Self {
        Self { 
            poll_interval: Duration::from_secs(30),
            compare_contents: false
        }
    }
}