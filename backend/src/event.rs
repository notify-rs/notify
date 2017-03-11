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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventKind {
    Any,
    Access(AccessKind),
    Create(CreateKind),
    Modify(ModifyKind),
    Remove(RemoveKind),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    pub kind: EventKind,
    pub paths: Vec<PathBuf>,
    pub cookie: Option<u32>,
}

