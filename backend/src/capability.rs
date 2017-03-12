#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Capability {
    EmitOnAccess,
    TrackRelated,
    WatchEntireFilesystem,
    WatchFiles,
    WatchFolders,
    WatchNewFolders,
    WatchRecursively,
}
