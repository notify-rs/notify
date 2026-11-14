//! Error types

use crate::{Config, PathOp};
use std::error::Error as StdError;
use std::fmt::Debug;
use std::path::PathBuf;
use std::result::Result as StdResult;
use std::{self, fmt, io};

/// Type alias to use this library's `Error` type in a Result
pub type Result<T> = StdResult<T, Error>;

/// Error kinds
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Generic error
    ///
    /// May be used in cases where a platform specific error is mapped to this type, or for opaque
    /// internal errors.
    Generic(String),

    /// I/O errors.
    Io(io::Error),

    /// A path does not exist.
    PathNotFound,

    /// Attempted to remove a watch that does not exist.
    WatchNotFound,

    /// An invalid value was passed as runtime configuration.
    InvalidConfig(Config),

    /// A watch limit was reached: an inotify watch limit, a kqueue descriptor limit, or
    /// notify's [FSEvents root limit](crate#macos-fsevents-limit-on-watched-paths).
    MaxFilesWatch,
}

/// Notify error type.
///
/// Errors are emitted either at creation time of a `Watcher`, or during the event stream. They
/// range from kernel errors to filesystem errors to argument errors.
///
/// Errors can be general, or they can be about specific paths or subtrees. In that later case, the
/// error's `paths` field will be populated.
#[derive(Debug)]
pub struct Error {
    /// Kind of the error.
    pub kind: ErrorKind,

    /// Relevant paths to the error, if any.
    pub paths: Vec<PathBuf>,

    user_data: Option<u64>,
}

impl Error {
    /// Returns the application-defined value associated with this error.
    ///
    /// Errors returned by [`crate::Watcher::watch_with`] carry the value from its configuration.
    /// For a failed watch operation in [`crate::Watcher::update_paths`], the source error carries
    /// that operation's value. Errors delivered to the event handler carry the reporting watch's
    /// value, including errors encountered while installing recursive watches.
    ///
    /// `None` means no value was configured or the error has no specific watch or watch operation.
    /// `Some(value)` does not guarantee that other watches are unaffected.
    #[must_use]
    pub fn user_data(&self) -> Option<u64> {
        self.user_data
    }

    /// Sets [`Error::user_data`].
    #[must_use]
    pub fn set_user_data(self, user_data: u64) -> Self {
        self.set_user_data_opt(Some(user_data))
    }

    pub(crate) fn set_user_data_opt(mut self, user_data: Option<u64>) -> Self {
        self.user_data = user_data;
        self
    }

    /// Adds a path to the error.
    #[must_use]
    pub fn add_path(mut self, path: PathBuf) -> Self {
        self.paths.push(path);
        self
    }

    /// Replaces the paths for the error.
    #[must_use]
    pub fn set_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.paths = paths;
        self
    }

    /// Creates a new Error with empty paths given its kind.
    #[must_use]
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            paths: Vec::new(),
            user_data: None,
        }
    }

    /// Creates a new generic Error from a message.
    #[must_use]
    pub fn generic(msg: &str) -> Self {
        Self::new(ErrorKind::Generic(msg.into()))
    }

    /// Creates a new i/o Error from a stdlib `io::Error`.
    #[must_use]
    pub fn io(err: io::Error) -> Self {
        Self::new(ErrorKind::Io(err))
    }

    /// Similar to [`Error::io`], but specifically handles [`io::ErrorKind::NotFound`].
    #[must_use]
    pub fn io_watch(err: io::Error) -> Self {
        if err.kind() == io::ErrorKind::NotFound {
            Self::path_not_found()
        } else {
            Self::io(err)
        }
    }

    /// Creates a new "path not found" error.
    #[must_use]
    pub fn path_not_found() -> Self {
        Self::new(ErrorKind::PathNotFound)
    }

    /// Creates a new "watch not found" error.
    #[must_use]
    pub fn watch_not_found() -> Self {
        Self::new(ErrorKind::WatchNotFound)
    }

    /// Creates a new "invalid config" error from the given `Config`.
    #[must_use]
    pub fn invalid_config(config: &Config) -> Self {
        Self::new(ErrorKind::InvalidConfig(*config))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let error = match self.kind {
            ErrorKind::PathNotFound => "No path was found.".into(),
            ErrorKind::WatchNotFound => "No watch was found.".into(),
            ErrorKind::InvalidConfig(ref config) => format!("Invalid configuration: {config:?}"),
            ErrorKind::Generic(ref err) => err.clone(),
            ErrorKind::Io(ref err) => err.to_string(),
            ErrorKind::MaxFilesWatch => "OS file watch limit reached.".into(),
        };

        if self.paths.is_empty() {
            write!(f, "{error}")
        } else {
            write!(f, "{} about {:?}", error, self.paths)
        }
    }
}

impl StdError for Error {
    fn cause(&self) -> Option<&dyn StdError> {
        match self.kind {
            ErrorKind::Io(ref cause) => Some(cause),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::io(err)
    }
}

impl<T> From<std::sync::mpsc::SendError<T>> for Error {
    fn from(err: std::sync::mpsc::SendError<T>) -> Self {
        Error::generic(&format!("internal channel disconnect: {err:?}"))
    }
}

impl From<std::sync::mpsc::RecvError> for Error {
    fn from(err: std::sync::mpsc::RecvError) -> Self {
        Error::generic(&format!("internal channel disconnect: {err:?}"))
    }
}

impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(err: std::sync::PoisonError<T>) -> Self {
        Error::generic(&format!("internal mutex poisoned: {err:?}"))
    }
}

/// The error provided by [`crate::Watcher::update_paths`] method.
///
/// Operations are applied in order. If an error occurs, processing stops and the
/// error carries the failed operation (if known) and any remaining operations that
/// were not attempted.
#[derive(Debug)]
#[non_exhaustive]
pub struct UpdatePathsError {
    /// The original error
    pub source: Error,

    /// The operation that caused the error.
    ///
    /// If set, all operations before it were applied successfully.
    /// `None` if the error was not caused by a specific operation
    /// (e.g. failure to start the watcher after successfully updating paths).
    pub origin: Option<PathOp>,

    /// The remaining operations that haven't been applied.
    ///
    /// This list does not include `origin`. To retry in order, handle `origin`
    /// first (if present), then `remaining`.
    pub remaining: Vec<PathOp>,
}

impl UpdatePathsError {
    pub(crate) fn with_origin_user_data(mut self) -> Self {
        if let Some(PathOp::Watch(_, config)) = &self.origin {
            self.source = self.source.set_user_data_opt(config.user_data());
        }
        self
    }

    /// Creates a batch update error.
    ///
    /// `origin` is the failed operation, if known. `remaining` contains only operations
    /// that were not attempted and must not include `origin`.
    ///
    /// # Examples
    ///
    /// ```
    /// use notify::{Error, PathOp, UpdatePathsError};
    ///
    /// let error = UpdatePathsError::new(
    ///     Error::watch_not_found(),
    ///     Some(PathOp::unwatch("missing")),
    ///     vec![PathOp::watch_recursive("next")],
    /// );
    /// ```
    #[must_use]
    pub fn new(source: Error, origin: Option<PathOp>, remaining: Vec<PathOp>) -> Self {
        Self {
            source,
            origin,
            remaining,
        }
    }
}

impl fmt::Display for UpdatePathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unable to apply the batch operation: {}", self.source)
    }
}

impl StdError for UpdatePathsError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.source)
    }
}

impl From<UpdatePathsError> for Error {
    fn from(value: UpdatePathsError) -> Self {
        value.source
    }
}

impl IntoIterator for UpdatePathsError {
    type Item = PathOp;

    type IntoIter = std::iter::Chain<std::option::IntoIter<PathOp>, std::vec::IntoIter<PathOp>>;

    fn into_iter(self) -> Self::IntoIter {
        self.origin.into_iter().chain(self.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn user_data_preserves_error_details() {
        let error = Error::io(io::Error::from_raw_os_error(1)).add_path("watched".into());
        let display = error.to_string();
        assert_eq!(error.user_data(), None);
        for value in [0, u64::MAX] {
            let error = Error::io(io::Error::from_raw_os_error(1))
                .add_path("watched".into())
                .set_user_data(value);
            assert_eq!(error.user_data(), Some(value));
            assert_eq!(error.to_string(), display);
            assert!(matches!(&error.kind, ErrorKind::Io(err) if err.raw_os_error() == Some(1)));
            assert!(format!("{error:?}").contains(&format!("user_data: Some({value})")));
        }
    }

    #[test]
    fn default_watch_with_replaces_the_inner_error_value() {
        use crate::{EventHandler, RecursiveMode, WatchPathConfig, Watcher, WatcherKind};

        struct FailingWatcher;
        impl Watcher for FailingWatcher {
            fn new<F: EventHandler>(_: F, _: Config) -> Result<Self> {
                Ok(Self)
            }

            fn watch(&mut self, _: &Path, _: RecursiveMode) -> Result<()> {
                Err(Error::path_not_found().set_user_data(42))
            }

            fn unwatch(&mut self, _: &Path) -> Result<()> {
                unreachable!()
            }

            fn kind() -> WatcherKind {
                WatcherKind::NullWatcher
            }
        }

        for value in [None, Some(0), Some(u64::MAX)] {
            let mut config = WatchPathConfig::new(RecursiveMode::Recursive);
            if let Some(value) = value {
                config = config.with_user_data(value);
            }
            let error = FailingWatcher
                .watch_with(Path::new("missing"), config)
                .unwrap_err();
            assert_eq!(error.user_data(), value);
        }
    }

    #[test]
    fn batch_errors_use_the_failed_watch_configuration() {
        use crate::{RecursiveMode, WatchPathConfig};

        for value in [None, Some(0), Some(u64::MAX)] {
            let mut config = WatchPathConfig::new(RecursiveMode::Recursive);
            if let Some(value) = value {
                config = config.with_user_data(value);
            }
            let error = crate::update_paths(
                vec![
                    PathOp::Watch("failed".into(), config),
                    PathOp::watch_recursive("remaining"),
                ],
                |op| Err((op, Error::path_not_found().set_user_data(42))),
            )
            .unwrap_err();
            assert_eq!(error.source.user_data(), value);
            assert!(
                matches!(&error.origin, Some(PathOp::Watch(path, _)) if path == Path::new("failed"))
            );
            assert_eq!(error.remaining.len(), 1);
            assert_eq!(error.remaining[0].as_path(), Path::new("remaining"));
            assert_eq!(Error::from(error).user_data(), value);
        }
    }

    #[test]
    fn batch_errors_without_a_watch_operation_preserve_the_source() {
        for origin in [None, Some(PathOp::unwatch("missing"))] {
            let error = UpdatePathsError::new(Error::watch_not_found(), origin, Vec::new())
                .with_origin_user_data();
            assert_eq!(error.source.user_data(), None);

            let error = UpdatePathsError::new(
                Error::generic("already attributed").set_user_data(42),
                error.origin,
                Vec::new(),
            )
            .with_origin_user_data();
            assert_eq!(error.source.user_data(), Some(42));
        }
    }

    #[test]
    fn display_formatted_errors() {
        let expected = "Some error";

        assert_eq!(expected, format!("{}", Error::generic(expected)));

        assert_eq!(
            expected,
            format!("{}", Error::io(io::Error::other(expected)))
        );
    }

    #[test]
    fn display_update_paths() {
        let actual = UpdatePathsError {
            source: Error::generic("Some error"),
            origin: None,
            remaining: Default::default(),
        }
        .to_string();

        assert_eq!(
            format!("unable to apply the batch operation: Some error"),
            actual
        );
    }
}
