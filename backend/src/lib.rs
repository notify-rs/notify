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
//! extern crate notify_backend as backend;
//!
//! use backend::prelude::*;
//! ```
//!
//! And optionally the Buffer:
//!
//! ```rust,ignore
//! use backend::Buffer;
//! ```
//!
//! The prelude imports all types needed to implement a Backend. Refer to the [implementor's guide]
//! for a thorough walk-through of backend implementation.

#![deny(missing_docs)]

pub extern crate futures;
pub extern crate mio;

pub use self::buffer::Buffer;

pub mod backend;
pub mod buffer;
pub mod capability;
pub mod event;
pub mod stream;

#[macro_use] pub mod compliance;

/// The Notify prelude.
///
/// All that is needed to implement a `Backend`, except for the optional `Buffer`.
pub mod prelude {
    pub use futures::{self,
        Future,
        Poll,
        Stream,
    };

    pub use mio::{self,
        Evented,
        Poll as MioPoll,
        PollOpt as MioPollOpt,
        Ready as MioReady,
        Token as MioToken
    };

    pub use std::path::PathBuf;

    /// An empty io::Result used for mio's Evented trait signatures
    pub type MioResult = ::std::io::Result<()>;

    pub use super::backend::{
        Backend as NotifyBackend,
        BoxedBackend,
        Error as BackendError,
        Result as BackendResult,
        SyncResult as BackendSyncResult,
    };

    pub use super::capability::Capability;

    pub use super::event::{
        AccessKind,
        AccessMode,
        CreateKind,
        DataChange,
        Event,
        EventKind,
        MetadataKind,
        ModifyKind,
        RemoveKind,
        RenameMode,
    };

    pub use super::stream::{
        Error as StreamError,
        Item as StreamItem,
        EmptyResult as EmptyStreamResult,
    };
}
