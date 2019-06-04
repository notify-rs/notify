#![allow(missing_docs)]

pub use self::op::Op;
pub use std::path::PathBuf;
use crate::Result;

pub mod op {
    bitflags::bitflags! {
        pub struct Op: u32 {
            const METADATA       = 0b0000001;
            const CREATE      = 0b0000010;
            const REMOVE      = 0b0000100;
            const RENAME      = 0b0001000;
            const WRITE       = 0b0010000;
            const CLOSE_WRITE = 0b0100000;
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

#[derive(Debug)]
pub struct RawEvent {
    /// Path where the event originated.
    ///
    /// `path` is always absolute, even if a relative path is used to watch a file or directory.
    ///
    /// On **macOS** the path is always canonicalized.
    ///
    /// Keep in mind that the path may be `None` if the watched file or directory or any parent
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
