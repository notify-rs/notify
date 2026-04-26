use std::{
    path::{Path, PathBuf},
    time::Instant,
};

/// An entry that holds back events at one path until an event at another path is emitted.
///
/// Used to prevent events queued for a renamed path from being emitted before
/// the [`RenameMode::Both`](notify::event::RenameMode) event at the new path
/// has itself been emitted.
#[derive(Debug)]
pub struct BlockEntry {
    /// Path whose queued event must be emitted first (the rename destination).
    pub blocker_path: PathBuf,
    /// Timestamp of the blocking event, used to match the exact rename event.
    pub blocker_time: Instant,
    /// Path whose events are held back until the blocker is emitted (the rename source).
    pub blockee_path: PathBuf,
}

/// Tracks which paths have their event emission held back by a pending rename.
///
/// When a rename `A → B` is queued, events already in A's queue must not be
/// emitted before the rename event at B. `BlockManager` records this
/// dependency so [`debounced_events`] can skip blocked paths until the
/// rename at the blocker path is emitted and [`remove_blocker`] is called.
///
/// [`debounced_events`]: crate::DebounceDataInner::debounced_events
/// [`remove_blocker`]: BlockManager::remove_blocker
#[derive(Debug, Default)]
pub struct BlockManager {
    pub entries: Vec<BlockEntry>,
}

impl BlockManager {
    /// Construct an empty `BlockManager`.
    pub fn new() -> BlockManager {
        Self::default()
    }

    /// Register a new blocking relationship.
    pub fn add_blocker(&mut self, entry: BlockEntry) {
        self.entries.push(entry);
    }

    /// Remove the blocker for `path` at `time` once its event has been emitted.
    pub fn remove_blocker(&mut self, path: &Path, time: Instant) {
        self.entries
            .retain(|entry| entry.blocker_path != *path || entry.blocker_time != time);
    }

    /// Return the blocker for `path`, if one exists.
    ///
    /// Returns `(blocker_path, blocker_time)` when `path`'s events are held
    /// back, or `None` if `path` is not blocked.
    pub fn is_blocked_by(&self, path: &Path) -> Option<(&PathBuf, Instant)> {
        self.entries
            .iter()
            .find(|entry| entry.blockee_path == *path)
            .map(|entry| (&entry.blocker_path, entry.blocker_time))
    }
}
