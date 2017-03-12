use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessMode {
    Any,
    Execute,
    Read,
    Write,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessKind {
    Any,
    Read,
    Open(AccessMode),
    Close(AccessMode),
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreateKind {
    Any,
    File,
    Folder,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataChange {
    Any,
    Size,
    Content,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataKind {
    Any,
    AccessTime,
    WriteTime,
    Permissions,
    Ownership,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenameMode {
    Any,
    To,
    From,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModifyKind {
    Any,
    Data(DataChange),
    Metadata(MetadataKind),
    Name(RenameMode),
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoveKind {
    Any,
    File,
    Folder,
    Other(String),
}

/// Top-level event kind.
///
/// This is arguably the most important classification for events. All subkinds
/// below this one represent details that may or may not be available for any
/// particular backend, but most tools and Notify systems will only care about
/// which of these four general kinds an event is about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventKind {
    /// The catch-all event kind, for unsupported/unknown events.
    ///
    /// This variant should be used as the "else" case when mapping native
    /// kernel bitmasks or bitmaps, such that if the mask is ever extended with
    /// new event types the backend will not gain bugs due to not matching new
    /// unknown event types.
    Any,

    /// An event describing non-mutating access operations on files.
    ///
    /// This event is about opening and closing file handles, as well as
    /// executing files, and any other such event that is about accessing
    /// files, folders, or other structures rather than mutating them.
    ///
    /// Only backends with the `EmitOnAccess` capability will generate these.
    Access(AccessKind),

    /// An event describing creation operations on files.
    ///
    /// This event is about the creation of files, folders, or other structures
    /// but not about e.g. writing new content into them.
    Create(CreateKind),

    /// An event describing mutation of content, name, or metadata.
    ///
    /// This event is about the mutation of files', folders', or other
    /// structures' content, name (path), or associated metadata (attributes).
    Modify(ModifyKind),

    /// An event describing removal operations on files.
    ///
    /// This event is about the removal of files, folders, or other structures
    /// but not e.g. erasing content from them. This may also be triggered for
    /// renames/moves that move files _out of the watched subpath_.
    ///
    /// Some editors also trigger Remove events when saving files as they may
    /// opt for removing (or renaming) the original then creating a new file
    /// in-place.
    Remove(RemoveKind),

    /// An event not fitting in any of the above four categories.
    ///
    /// This may be used for meta-events about the watch itself, but generally
    /// should not be used.
    Other(String),
}

impl EventKind {
    pub fn is_access(&self) -> bool {
        match self {
            &EventKind::Access(_) => true,
            _ => false
        }
    }

    pub fn is_create(&self) -> bool {
        match self {
            &EventKind::Create(_) => true,
            _ => false
        }
    }

    pub fn is_modify(&self) -> bool {
        match self {
            &EventKind::Modify(_) => true,
            _ => false
        }
    }

    pub fn is_remove(&self) -> bool {
        match self {
            &EventKind::Remove(_) => true,
            _ => false
        }
    }
}

/// Notify event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    /// Kind of the event.
    ///
    /// This is a hierarchy of enums describing the event as precisely as
    /// possible. All enums in the hierarchy have two variants always present,
    /// `Any` and `Other(String)`, accompanied by one or more specific variants.
    ///
    /// `Any` should be used when more detail about the event is not known
    /// beyond the variant already selected. For example, `AccessMode::Any`
    /// means a file has been accessed, but that's all we know.
    ///
    /// `Other` should be used when more detail _is_ available, but cannot be
    /// encoded as one of the defined variants. For example,
    /// `CreateKind::Other("mount")` may indicate the binding of a mount. The
    /// documentation of the particular backend should indicate if any `Other`
    /// events are generated, and what their description means.
    ///
    /// The `EventKind::Any` variant should be used as the "else" case when
    /// mapping native kernel bitmasks or bitmaps, such that if the mask is ever
    /// extended with new event types the backend will not gain bugs due to not
    /// matching new unknown event types.
    pub kind: EventKind,

    /// Paths that the event is about.
    ///
    /// Generally that will be a single path, but it may be more in backends
    /// that track e.g. renames by path instead of "cookie".
    pub paths: Vec<PathBuf>,

    /// Relation ID for events that are related.
    ///
    /// This will only be `Some` for events generated by backends with the
    /// `TrackRelated` capability. Those backends _may_ emit events that are
    /// related to each other, and tag those with an identical `relid` or
    /// "cookie". The value is normalised to `usize`.
    pub relid: Option<usize>,
}

