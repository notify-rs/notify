#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Capability {
    WatchFiles,
    WatchFolders,
    WatchRecursively,
    WatchEntireFilesystem,
    // TODO: TrackOpen, TrackModify, TrackAttributes, etc
}
