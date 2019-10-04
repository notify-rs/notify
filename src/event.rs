// This file is dual-licensed under the Artistic License 2.0 as per the
// LICENSE.ARTISTIC file, and the Creative Commons Zero 1.0 license.
//! The `Event` type and the hierarchical `EventKind` descriptor.

use anymap::{any::CloneAny, Map};
use std::{
    fmt,
    hash::{Hash, Hasher},
    path::PathBuf,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// An `AnyMap` convenience type with the needed bounds for events.
pub type AnyMap = Map<dyn CloneAny + Send + Sync>;

/// An event describing open or close operations on files.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum AccessMode {
    /// The catch-all case, to be used when the specific kind of event is unknown.
    Any,

    /// An event emitted when the file is executed, or the folder opened.
    Execute,

    /// An event emitted when the file is opened for reading.
    Read,

    /// An event emitted when the file is opened for writing.
    Write,

    /// An event which specific kind is known but cannot be represented otherwise.
    Other,
}

/// An event describing non-mutating access operations on files.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", content = "mode"))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum AccessKind {
    /// The catch-all case, to be used when the specific kind of event is unknown.
    Any,

    /// An event emitted when the file is read.
    Read,

    /// An event emitted when the file, or a handle to the file, is opened.
    Open(AccessMode),

    /// An event emitted when the file, or a handle to the file, is closed.
    Close(AccessMode),

    /// An event which specific kind is known but cannot be represented otherwise.
    Other,
}

/// An event describing creation operations on files.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind"))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum CreateKind {
    /// The catch-all case, to be used when the specific kind of event is unknown.
    Any,

    /// An event which results in the creation of a file.
    File,

    /// An event which results in the creation of a folder.
    Folder,

    /// An event which specific kind is known but cannot be represented otherwise.
    Other,
}

/// An event emitted when the data content of a file is changed.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum DataChange {
    /// The catch-all case, to be used when the specific kind of event is unknown.
    Any,

    /// An event emitted when the size of the data is changed.
    Size,

    /// An event emitted when the content of the data is changed.
    Content,

    /// An event which specific kind is known but cannot be represented otherwise.
    Other,
}

/// An event emitted when the metadata of a file or folder is changed.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum MetadataKind {
    /// The catch-all case, to be used when the specific kind of event is unknown.
    Any,

    /// An event emitted when the access time of the file or folder is changed.
    AccessTime,

    /// An event emitted when the write or modify time of the file or folder is changed.
    WriteTime,

    /// An event emitted when the permissions of the file or folder are changed.
    Permissions,

    /// An event emitted when the ownership of the file or folder is changed.
    Ownership,

    /// An event emitted when an extended attribute of the file or folder is changed.
    ///
    /// If the extended attribute's name or type is known, it should be provided in the
    /// `Info` event attribute.
    Extended,

    /// An event which specific kind is known but cannot be represented otherwise.
    Other,
}

/// An event emitted when the name of a file or folder is changed.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum RenameMode {
    /// The catch-all case, to be used when the specific kind of event is unknown.
    Any,

    /// An event emitted on the file or folder resulting from a rename.
    To,

    /// An event emitted on the file or folder that was renamed.
    From,

    /// A single event emitted with both the `From` and `To` paths.
    ///
    /// This event should be emitted when both source and target are known. The paths should be
    /// provided in this exact order (from, to).
    Both,

    /// An event which specific kind is known but cannot be represented otherwise.
    Other,
}

/// An event describing mutation of content, name, or metadata.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", content = "mode"))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum ModifyKind {
    /// The catch-all case, to be used when the specific kind of event is unknown.
    Any,

    /// An event emitted when the data content of a file is changed.
    Data(DataChange),

    /// An event emitted when the metadata of a file or folder is changed.
    Metadata(MetadataKind),

    /// An event emitted when the name of a file or folder is changed.
    #[cfg_attr(feature = "serde", serde(rename = "rename"))]
    Name(RenameMode),

    /// An event which specific kind is known but cannot be represented otherwise.
    Other,
}

/// An event describing removal operations on files.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind"))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum RemoveKind {
    /// The catch-all case, to be used when the specific kind of event is unknown.
    Any,

    /// An event emitted when a file is removed.
    File,

    /// An event emitted when a folder is removed.
    Folder,

    /// An event which specific kind is known but cannot be represented otherwise.
    Other,
}

/// Top-level event kind.
///
/// This is arguably the most important classification for events. All subkinds below this one
/// represent details that may or may not be available for any particular backend, but most tools
/// and Notify systems will only care about which of these four general kinds an event is about.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum EventKind {
    /// The catch-all event kind, for unsupported/unknown events.
    ///
    /// This variant should be used as the "else" case when mapping native kernel bitmasks or
    /// bitmaps, such that if the mask is ever extended with new event types the backend will not
    /// gain bugs due to not matching new unknown event types.
    ///
    /// This variant is also the default variant used when Notify is in "imprecise" mode.
    Any,

    /// An event describing non-mutating access operations on files.
    ///
    /// This event is about opening and closing file handles, as well as executing files, and any
    /// other such event that is about accessing files, folders, or other structures rather than
    /// mutating them.
    ///
    /// Only some platforms are capable of generating these.
    Access(AccessKind),

    /// An event describing creation operations on files.
    ///
    /// This event is about the creation of files, folders, or other structures but not about e.g.
    /// writing new content into them.
    Create(CreateKind),

    /// An event describing mutation of content, name, or metadata.
    ///
    /// This event is about the mutation of files', folders', or other structures' content, name
    /// (path), or associated metadata (attributes).
    Modify(ModifyKind),

    /// An event describing removal operations on files.
    ///
    /// This event is about the removal of files, folders, or other structures but not e.g. erasing
    /// content from them. This may also be triggered for renames/moves that move files _out of the
    /// watched subpath_.
    ///
    /// Some editors also trigger Remove events when saving files as they may opt for removing (or
    /// renaming) the original then creating a new file in-place.
    Remove(RemoveKind),

    /// An event not fitting in any of the above four categories.
    ///
    /// This may be used for meta-events about the watch itself. In "imprecise" mode, it is, along
    /// with `Any`, the only other event generated.
    Other,
}

impl EventKind {
    /// Indicates whether an event is an Access variant.
    pub fn is_access(&self) -> bool {
        match *self {
            EventKind::Access(_) => true,
            _ => false,
        }
    }

    /// Indicates whether an event is a Create variant.
    pub fn is_create(&self) -> bool {
        match *self {
            EventKind::Create(_) => true,
            _ => false,
        }
    }

    /// Indicates whether an event is a Modify variant.
    pub fn is_modify(&self) -> bool {
        match *self {
            EventKind::Modify(_) => true,
            _ => false,
        }
    }

    /// Indicates whether an event is a Remove variant.
    pub fn is_remove(&self) -> bool {
        match *self {
            EventKind::Remove(_) => true,
            _ => false,
        }
    }

    /// Indicates whether an event is an Other variant.
    pub fn is_other(&self) -> bool {
        match *self {
            EventKind::Other => true,
            _ => false,
        }
    }
}

impl Default for EventKind {
    fn default() -> Self {
        EventKind::Any
    }
}

/// Notify event.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Event {
    /// Kind or type of the event.
    ///
    /// This is a hierarchy of enums describing the event as precisely as possible. All enums in
    /// the hierarchy have two variants always present, `Any` and `Other`, accompanied by one or
    /// more specific variants.
    ///
    /// `Any` should be used when more detail about the event is not known beyond the variant
    /// already selected. For example, `AccessMode::Any` means a file has been accessed, but that's
    /// all we know.
    ///
    /// `Other` should be used when more detail _is_ available, but cannot be encoded as one of the
    /// defined variants. When specifying `Other`, the event attributes should contain an `Info`
    /// entry with a short string identifying this detail. That string is to be considered part of
    /// the interface of the backend (i.e. a change should probably be breaking).
    ///
    /// For example, `CreateKind::Other` with an `Info("mount")` may indicate the binding of a
    /// mount. The documentation of the particular backend should indicate if any `Other` events
    /// are generated, and what their description means.
    ///
    /// The `EventKind::Any` variant should be used as the "else" case when mapping native kernel
    /// bitmasks or bitmaps, such that if the mask is ever extended with new event types the
    /// backend will not gain bugs due to not matching new unknown event types.
    #[cfg_attr(feature = "serde", serde(rename = "type"))]
    pub kind: EventKind,

    /// Paths the event is about, if known.
    ///
    /// If an event concerns two or more paths, and the paths are known at the time of event
    /// creation, they should all go in this `Vec`. Otherwise, using the `Tracker` attr may be more
    /// appropriate.
    ///
    /// The order of the paths is likely to be significant! For example, renames where both ends of
    /// the name change are known will have the "source" path first, and the "target" path last.
    pub paths: Vec<PathBuf>,

    // "What should be in the struct" and "what can go in the attrs" is an interesting question.
    //
    // Technically, the paths could go in the attrs. That would reduce the type size to 4 pointer
    // widths, instead of 7 like it is now. Anything 8 and below is probably good — on x64 that's
    // the size of an L1 cache line. The entire kind classification fits in 3 bytes, and an AnyMap
    // is 3 pointers. A Vec<PathBuf> is another 3 pointers.
    //
    // Type size aside, what's behind these structures? A Vec and a PathBuf is stored on the heap.
    // An AnyMap is stored on the heap. But a Vec is directly there, requiring about one access to
    // get, while retrieving anything in the AnyMap requires some accesses as overhead.
    //
    // So things that are used often should be on the struct, and things that are used more rarely
    // should go in the attrs. Additionally, arbitrary data can _only_ go in the attrs.
    //
    // The kind and the paths vie for first place on this scale, depending on how downstream wishes
    // to use the information. Everything else is secondary. So far, that's why paths live here.
    //
    // In the future, it might be possible to have more data and to benchmark things properly, so
    // the perfomance can be actually quantified. Also, it might turn out that I have no idea what
    // I was talking about, so the above may be discarded or reviewed. We'll see!
    //
    /// Additional attributes of the event.
    ///
    /// Arbitrary data may be added to this field, without restriction beyond the `Sync` and
    /// `Clone` properties. Some data added here is considered for comparing and hashing, but not
    /// all: at this writing this is `Tracker`, `Flag`, `Info`, and `Source`.
    ///
    /// For vendor or custom information, it is recommended to use type wrappers to differentiate
    /// entries within the `AnyMap` container and avoid conflicts. For interoperability, one of the
    /// “well-known” types (or propose a new one) should be used instead. See the list on the wiki:
    /// https://github.com/notify-rs/notify/wiki/Well-Known-Event-Attrs
    ///
    /// There is currently no way to serialise or deserialise arbitrary attributes, only well-known
    /// ones handled explicitly here are supported. This might change in the future.
    #[cfg_attr(feature = "serde", serde(default = "AnyMap::new"))]
    #[cfg_attr(feature = "serde", serde(deserialize_with = "attr_serde::deserialize"))]
    #[cfg_attr(feature = "serde", serde(serialize_with = "attr_serde::serialize"))]
    pub attrs: AnyMap,
}

/// Tracking ID for events that are related.
///
/// For events generated by backends with the `TrackRelated` capability. Those backends _may_ emit
/// events that are related to each other, and tag those with an identical "tracking id" or
/// "cookie". The value is normalised to `usize`.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Deserialize))]
pub struct Tracker(pub usize);

/// Special Notify flag on the event.
///
/// This attribute is used to flag certain kinds of events that Notify either marks or generates in
/// particular ways.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub enum Flag {
    /*
        /// Event notices are emitted by debounced watchers immediately after the _first_ event of that
        /// kind is received on a path to indicate activity to a path within the interval of a debounce.
        ///
        /// Event notices are a runtime option and are disabled by default. (TODO)
        Notice,

        /// Ongoing event notices are emitted by debounced watchers on a higher frequency than the
        /// debouncing delay to indicate ongoing activity to a path within the interval of a debounce.
        ///
        /// Ongoing event notices are a runtime option and are disabled by default.
        Ongoing,
    */
    /// Rescan notices are emitted by some platforms (and may also be emitted by Notify itself).
    /// They indicate either a lapse in the events or a change in the filesystem such that events
    /// received so far can no longer be relied on to represent the state of the filesystem now.
    ///
    /// An application that simply reacts to file changes may not care about this. An application
    /// that keeps an in-memory representation of the filesystem will need to care, and will need
    /// to refresh that representation directly from the filesystem.
    Rescan,
}

/// Additional information on the event.
///
/// This is to be used for all `Other` variants of the event kind hierarchy. The variant indicates
/// that a consumer should look into the `attrs` for an `Info` value; if that value is missing it
/// should be considered a backend bug.
///
/// This attribute may also be present for non-`Other` variants of the event kind, if doing so
/// provides useful precision. For example, the `Modify(Metadata(Extended))` kind suggests using
/// this attribute when information about _what_ extended metadata changed is available.
///
/// This should be a short string, and changes may be considered breaking.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Deserialize))]
pub struct Info(pub String);

/// The source of the event.
///
/// In most cases this should be a short string, identifying the backend unambiguously. In some
/// cases this may be dynamically generated, but should contain a prefix to make it unambiguous
/// between backends.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Deserialize))]
pub struct Source(pub String);

/// The process ID of the originator of the event.
///
/// This attribute is experimental and, while included in Notify itself, is not considered stable
/// or standard enough to be part of the serde, eq, hash, and debug representations.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Deserialize))]
pub struct ProcessID(pub u32);

// + typeid attr?

impl Event {
    /// Retrieves the tracker ID for an event directly, if present.
    pub fn tracker(&self) -> Option<usize> {
        self.attrs.get::<Tracker>().map(|v| v.0)
    }

    /// Retrieves the Notify flag for an event directly, if present.
    pub fn flag(&self) -> Option<Flag> {
        self.attrs.get::<Flag>().cloned()
    }

    /// Retrieves the additional info for an event directly, if present.
    pub fn info(&self) -> Option<&String> {
        self.attrs.get::<Info>().map(|v| &v.0)
    }

    /// Retrieves the source for an event directly, if present.
    pub fn source(&self) -> Option<&String> {
        self.attrs.get::<Source>().map(|v| &v.0)
    }

    /// Creates a new `Event` given a kind.
    pub fn new(kind: EventKind) -> Self {
        Self {
            kind,
            paths: Vec::new(),
            attrs: AnyMap::new(),
        }
    }

    /// Sets the kind.
    pub fn set_kind(mut self, kind: EventKind) -> Self {
        self.kind = kind;
        self
    }

    /// Adds a path to the event.
    pub fn add_path(mut self, path: PathBuf) -> Self {
        self.paths.push(path);
        self
    }

    /// Adds a path to the event if the argument is Some.
    pub fn add_some_path(self, path: Option<PathBuf>) -> Self {
        if let Some(path) = path {
            self.add_path(path)
        } else {
            self
        }
    }

    /// Sets the tracker.
    pub fn set_tracker(mut self, tracker: usize) -> Self {
        self.attrs.insert(Tracker(tracker));
        self
    }

    /// Sets additional info onto the event.
    pub fn set_info(mut self, info: &str) -> Self {
        self.attrs.insert(Info(info.into()));
        self
    }

    /// Sets the Notify flag onto the event.
    pub fn set_flag(mut self, flag: Flag) -> Self {
        self.attrs.insert(flag);
        self
    }
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Event")
            .field("kind", &self.kind)
            .field("paths", &self.paths)
            .field("attr:tracker", &self.tracker())
            .field("attr:flag", &self.flag())
            .field("attr:info", &self.info())
            .field("attr:source", &self.source())
            .finish()
    }
}
impl Default for Event {
    fn default() -> Self {
        Self {
            kind: EventKind::default(),
            paths: Vec::new(),
            attrs: AnyMap::new(),
        }
    }
}

impl Eq for Event {}
impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.kind.eq(&other.kind)
            && self.paths.eq(&other.paths)
            && self.tracker().eq(&other.tracker())
            && self.flag().eq(&other.flag())
            && self.info().eq(&other.info())
            && self.source().eq(&other.source())
    }
}

impl Hash for Event {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.kind.hash(state);
        self.paths.hash(state);
        self.tracker().hash(state);
        self.flag().hash(state);
        self.info().hash(state);
        self.source().hash(state);
    }
}

#[cfg(feature = "serde")]
mod attr_serde {
    use super::*;
    use serde::{
        de::Deserializer,
        ser::{SerializeMap, Serializer},
    };
    use std::collections::HashMap;

    pub(crate) fn serialize<S>(attrs: &AnyMap, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let tracker = attrs.get::<Tracker>().map(|v| v.0.clone());
        let flag = attrs.get::<Flag>().map(|v| v.clone());
        let info = attrs.get::<Info>().map(|v| v.0.clone());
        let source = attrs.get::<Source>().map(|v| v.0.clone());

        let mut length = 0;
        if tracker.is_some() {
            length += 1;
        }
        if flag.is_some() {
            length += 1;
        }
        if info.is_some() {
            length += 1;
        }
        if source.is_some() {
            length += 1;
        }

        let mut map = s.serialize_map(Some(length))?;
        if let Some(val) = tracker {
            map.serialize_entry("tracker", &val)?;
        }
        if let Some(val) = flag {
            map.serialize_entry("flag", &val)?;
        }
        if let Some(val) = info {
            map.serialize_entry("info", &val)?;
        }
        if let Some(val) = source {
            map.serialize_entry("source", &val)?;
        }
        map.end()
    }

    pub(crate) fn deserialize<'de, D>(d: D) -> Result<AnyMap, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Clone, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        #[serde(untagged)]
        enum Attr {
            Tracker(Option<Tracker>),
            Flag(Option<Flag>),
            Info(Option<Info>),
            Source(Option<Source>),
        }

        #[derive(Deserialize)]
        struct Attrs(pub HashMap<String, Attr>);

        let attrs = Attrs::deserialize(d)?;
        let mut map = AnyMap::with_capacity(attrs.0.len());
        if let Some(Attr::Tracker(Some(val))) = attrs.0.get("tracker").cloned() {
            map.insert(val);
        }
        if let Some(Attr::Flag(Some(val))) = attrs.0.get("flag").cloned() {
            map.insert(val);
        }
        if let Some(Attr::Info(Some(val))) = attrs.0.get("info").cloned() {
            map.insert(val);
        }
        if let Some(Attr::Source(Some(val))) = attrs.0.get("source").cloned() {
            map.insert(val);
        }
        Ok(map)
    }
}
