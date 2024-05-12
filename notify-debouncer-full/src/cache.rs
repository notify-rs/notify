use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use file_id::{get_file_id, FileId};
use notify::RecursiveMode;
use walkdir::WalkDir;

/// The interface of a file ID cache.
///
/// This trait can be implemented for an existing cache, if it already holds `FileId`s.
pub trait FileIdCache {
    /// Get a `FileId` from the cache for a given `path`.
    ///
    /// If the path is not cached, `None` should be returned and there should not be any attempt to read the file ID from disk.
    fn cached_file_id(&self, path: &Path) -> Option<&FileId>;

    /// Add a new path to the cache or update its value.
    ///
    /// This will be called if a new file or directory is created or if an existing file is overridden.
    fn add_path(&mut self, path: &Path, recursive_mode: RecursiveMode);

    /// Remove a path from the cache.
    ///
    /// This will be called if a file or directory is deleted.
    fn remove_path(&mut self, path: &Path);

    /// Re-scan all paths.
    ///
    /// This will be called if the notification back-end has dropped events.
    fn rescan(&mut self, roots: &[(PathBuf, RecursiveMode)]) {
        for (root, recursive_mode) in roots {
            self.add_path(root, *recursive_mode);
        }
    }
}

/// A cache to hold the file system IDs of all watched files.
///
/// The file ID cache uses unique file IDs provided by the file system and is used to stich together
/// rename events in case the notification back-end doesn't emit rename cookies.
#[derive(Debug, Clone, Default)]
pub struct FileIdMap {
    paths: HashMap<PathBuf, FileId>,
}

impl FileIdMap {
    /// Construct an empty cache.
    pub fn new() -> Self {
        Default::default()
    }

    fn dir_scan_depth(is_recursive: bool) -> usize {
        if is_recursive {
            usize::MAX
        } else {
            1
        }
    }
}

impl FileIdCache for FileIdMap {
    fn cached_file_id(&self, path: &Path) -> Option<&FileId> {
        self.paths.get(path)
    }

    fn add_path(&mut self, path: &Path, recursive_mode: RecursiveMode) {
        let is_recursive = recursive_mode == RecursiveMode::Recursive;

        for (path, file_id) in WalkDir::new(path)
            .follow_links(true)
            .max_depth(Self::dir_scan_depth(is_recursive))
            .into_iter()
            .filter_map(|entry| {
                let path = entry.ok()?.into_path();
                let file_id = get_file_id(&path).ok()?;
                Some((path, file_id))
            })
        {
            self.paths.insert(path, file_id);
        }
    }

    fn remove_path(&mut self, path: &Path) {
        self.paths.retain(|p, _| !p.starts_with(path));
    }
}

/// An implementation of the `FileIdCache` trait that doesn't hold any data.
///
/// This pseudo cache can be used to disable the file tracking using file system IDs.
#[derive(Debug, Clone, Default)]
pub struct NoCache;

impl NoCache {
    /// Construct an empty cache.
    pub fn new() -> Self {
        Default::default()
    }
}

impl FileIdCache for NoCache {
    fn cached_file_id(&self, _path: &Path) -> Option<&FileId> {
        None
    }

    fn add_path(&mut self, _path: &Path, _recursive_mode: RecursiveMode) {}

    fn remove_path(&mut self, _path: &Path) {}
}

/// The recommended file ID cache implementation for the current platform
#[cfg(any(target_os = "linux", target_os = "android"))]
pub type RecommendedCache = NoCache;
/// The recommended file ID cache implementation for the current platform
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub type RecommendedCache = FileIdMap;
