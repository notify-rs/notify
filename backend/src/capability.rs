//! The `Capability` enum.

/// Codified descriptions of what backends can do themselves.
///
/// The idea is that Notify would very much like to have all or most of these things for all
/// platforms. However, making backends all do the same thing and provide the same behaviour
/// results in either **duplication** where missing duties are implemented several times in perhaps
/// different ways across backends, or **inefficiency** where backends that _can_ natively provide
/// some advanced behaviour are instead used minimally and the behaviour is then reimplemented
/// (often with bugs) on top of that.
///
/// The ideal solution is to have backends advertise which features they do support and only "fill
/// in" for them when they don't support something we require. Thus, these capabilities.
#[derive(Clone, Debug, Eq, PartialEq)] pub enum Capability {
    /// The backend generates `EventKind::Access` events.
    EmitOnAccess,

    /// The backend follows symlinks when watching files.
    FollowSymlinks,

    /// The backend emits events with the `relid` field.
    TrackRelated,

    /// The backend can be used to watch an entire mountpoint.
    ///
    /// This is a reserved capability as some native APIs are able to watch entire filesystems or
    /// mountpoints, but neither Notify nor the Backend interface really support this at the
    /// moment.
    WatchEntireFilesystem,

    /// The backend can watch individual files.
    ///
    /// This is to be taken to mean that it can be passed _a path to a file_ and will monitor
    /// changes to that file, without requiring to watch the containing directory.
    WatchFiles,

    /// The backend can watch entire folders.
    ///
    /// This is to be taken to mean that it can be passed _a path to a folder_ and will monitor
    /// changes to all files and folders within, without requiring to walk the folder and set up
    /// watches on individual entries within.
    ///
    /// This does not mean the backend can watch recursively.
    WatchFolders,

    /// The backend can watch new folders as they are created.
    ///
    /// This is to be taken to mean that when new folders are created within the path being
    /// watched, they in turn are watched, without requiring external intervention.
    ///
    /// This will almost always imply `WatchFolders` and `WatchRecursively`, but those capabilities
    /// should still be specified.
    WatchNewFolders,

    /// The backend can watch folders recursively.
    ///
    /// This is to be taken to mean that when asked to watch a folder, it will also watch all
    /// folders within, and folders within those, recursively.
    ///
    /// This will always imply `WatchFolders`, but that capability should still be specified.
    WatchRecursively,

    /// The backend can watch a networked filesystem.
    ///
    /// This is to be taken to mean that when a path refers to a networked filesystem, the backend
    /// can accommodate the increased latency and/or the circumstances and watch paths as or near
    /// as efficiently as usual.
    ///
    /// This is a reserved capability as some backends are able to satisfy these requirements, but
    /// Notify does not currently support this. However, it is planned to be used at some future.
    WatchNetworkedFilesystem,
}
