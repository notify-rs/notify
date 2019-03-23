//! The backend interface and utilities.
//!
//! This crate contains the `Backend` trait, the `Event` type, the `Capability` enum, and all other
//! types and utilities that are needed to implement a Notify backend.
//!
//! # Examples
//!
//! Implementors should start by including the prelude:
//!
//! ```rust,ignore
//! use notify_backend::prelude::*;
//! ```
//!
//! The prelude imports all types needed to implement a Backend, and re-exports dependent libraries
//! so there is no need to independently include them. Refer to the [implementor's guide] for a
//! thorough walk-through of backend implementation.
//!
//! [implementor's guide]: https://github.com/passcod/notify/wiki/Writing-a-Backend

#![feature(async_await, await_macro, futures_api)]

#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![deny(clippy::pedantic)]
#![allow(clippy::stutter)]

pub mod backend;
pub mod buffer;
pub mod capability;
pub mod event;
pub mod stream;

#[cfg(unix)]
pub mod unix;
// #[macro_use]
// pub mod compliance;

/// The Notify prelude.
///
/// All that is needed to implement a `Backend`, except for the optional `Buffer`.
pub mod prelude {
    pub use crate::buffer::Buffer;

    pub use futures::{self, Future, Poll, Stream};

    pub use mio::{
        self, event::Evented, Poll as MioPoll, PollOpt as MioPollOpt, Ready as MioReady,
        Registration as MioRegistration, Token as MioToken,
    };

    pub use std::path::PathBuf;

    /// An empty `io::Result` used for mio's Evented trait signatures
    pub type MioResult = ::std::io::Result<()>;

    pub use super::backend::{
        Backend as NotifyBackend, BoxedBackend, Error as BackendError,
        ErrorWrap as BackendErrorWrap, NewResult as NewBackendResult,
    };

    #[cfg(unix)]
    pub use crate::unix::OwnedEventedFd;

    pub use crate::capability::Capability;

    pub use crate::event::{
        self, AccessKind, AccessMode, CreateKind, DataChange, Event, EventKind,
        MetadataKind, ModifyKind, RemoveKind, RenameMode,
        // AnyMap,
    };

    pub use crate::stream::{
        EmptyResult as EmptyStreamResult, Error as StreamError, Item as StreamItem,
    };
}
