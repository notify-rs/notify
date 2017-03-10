use std::path::PathBuf;

// All event kinds have two variants always there: Any and Other(String). Any
// should be used when more details about the event is not known beyond the
// variant already selected. For example, AccessMode::Any means a file has been
// accessed, but that's all we know. Other should be used when more detail is
// available, but cannot be encoded as one of the defined variants. For example,
// CreateKind::Other("mount") may indicate the binding of a mount. The
// documentation of the particular backend should indicate if any Other events
// are generated, and what their description means.
//
// The EventKind::Any variant should be used when mapping native kernel types /
// bitmasks such that if the mask is ever extended with new event types the
// backend will still work.

pub enum AccessMode {
    Any,
    Execute,
    Read,
    Write,
    Other(String),
}

pub enum AccessKind {
    Any,
    Read,
    Open(AccessMode),
    Close(AccessMode),
    Other(String),
}

pub enum CreateKind {
    Any,
    File,
    Folder,
    Other(String),
}

pub enum DataChange {
    Any,
    Size,
    Content,
    Other(String),
}

pub enum MetadataKind {
    Any,
    AccessTime,
    WriteTime,
    Permissions,
    Ownership,
    Other(String),
}

pub enum RenameMode {
    Any,
    To,
    From,
    Other(String),
}

pub enum ModifyKind {
    Any,
    Data(DataChange),
    Metadata(MetadataKind),
    Name(RenameMode),
    Other(String),
}

pub enum RemoveKind {
    Any,
    File,
    Folder,
    Other(String),
}

pub enum EventKind {
    Any,
    Access(AccessKind),
    Create(CreateKind),
    Modify(ModifyKind),
    Remove(RemoveKind),
    Other(String),
}

pub struct Event {
    pub kind: EventKind,
    pub paths: Vec<PathBuf>,
    pub cookie: Option<u32>,
}

