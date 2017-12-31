//! The `Backend` trait and related types.

use futures::Stream;
use std::{ffi, io};
use std::path::PathBuf;
use std::result::Result as StdResult;
use super::capability::Capability;
use super::stream::{self, EmptyResult};

/// Convenient type alias for the Boxed Trait Object for Backend.
pub type BoxedBackend = Box<Backend<Item=stream::Item, Error=stream::Error>>;

/// A trait for types that implement Notify backends.
///
/// Be sure to thoroughly read the `Stream` documentation when implementing a `Backend`, as the
/// semantics described are relied upon by Notify, and incorrectly or incompletely implementing
/// them will result in bad behaviour.
///
/// Also take care to correctly free all resources via the `Drop` trait.
pub trait Backend: Stream + Drop {
    /// Creates an instance of a `Backend` that watches over a set of paths.
    ///
    /// While the `paths` argument is a `Vec` for implementation simplicity, Notify guarantees that
    /// it will only contain unique entries.
    ///
    /// This function must initialise all resources needed to watch over the paths, and only those
    /// paths. When the set of paths to be watched changes, the `Backend` will be `Drop`ped, and a
    /// new one recreated in its place. Thus, the `Backend` is immutable in this respect.
    fn new(paths: Vec<PathBuf>) -> Result<BoxedBackend> where Self: Sized;

    /// Returns the operational capabilities of this `Backend`.
    ///
    /// See the [`Capability` documentation][cap] for details.
    ///
    /// The function may perform checks and vary its response based on environmental factors.
    ///
    /// If the function returns an empty `Vec`, the `Backend` will be assumed to be inoperable at
    /// the moment (and another one may be selected). In general this should not happen, and
    /// instead an `Unavailable` error should be returned from `::new()`.
    ///
    /// [cap]: ../capability/enum.Capability.html
    fn capabilities(&self) -> Vec<Capability>;

    /// Blocks until events are available on this `Backend`.
    ///
    /// This should be implemented via kernel or native callbacks, and not via busy-wait or other
    /// infinite loops, unless that is the only way.
    fn await(&mut self) -> EmptyResult; }

/// A specialised Result for `Backend::new()`.
pub type Result<T: Backend> = StdResult<T, Error>;

/// Any error which may occur during the initialisation of a `Backend`.
#[derive(Debug)]
pub enum Error {
    /// An error represented by an arbitrary string.
    Generic(String),

    /// An I/O error.
    Io(io::Error),

    /// An error indicating that this Backend's implementation is incomplete.
    ///
    /// This is mostly to be used while developing Backends.
    NotImplemented,

    /// An error indicating that this Backend is unavailable, likely because its upstream or native
    /// API is inoperable. An optional reason may be supplied.
    Unavailable(Option<String>),

    /// An error indicating that one or more paths passed to the Backend do not exist. This should
    /// be translated from the native API or upstream's response: the frontend is responsible for
    /// pre-checking that paths exist.
    ///
    /// This error exists to cover cases where we lose a data race against the filesystem and the
    /// path is gone between the time the frontend checks it and the Backend initialises.
    ///
    /// It may contain the list of files that are reported to be non-existent if that is known.
    NonExistent(Vec<PathBuf>),

    /// An error indicating that one or more of the paths given is not supported by the `Backend`,
    /// with the relevant unsupported `Capability` passed along.
    NotSupported(Capability),

    /// A string conversion issue (nul byte found) from an FFI binding.
    FfiNul(ffi::NulError),

    /// A string conversion issue (UTF-8 error) from an FFI binding.
    FfiIntoString(ffi::IntoStringError),

    /// A str conversion issue (nul too early or absent) from an FFI binding.
    FfiFromBytes(ffi::FromBytesWithNulError)
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<Capability> for Error {
    fn from(cap: Capability) -> Self {
        Error::NotSupported(cap)
    }
}

impl From<ffi::NulError> for Error {
    fn from(err: ffi::NulError) -> Self {
        Error::FfiNul(err)
    }
}

impl From<ffi::IntoStringError> for Error {
    fn from(err: ffi::IntoStringError) -> Self {
        Error::FfiIntoString(err)
    }
}

impl From<ffi::FromBytesWithNulError> for Error {
    fn from(err: ffi::FromBytesWithNulError) -> Self {
        Error::FfiFromBytes(err)
    }
}
