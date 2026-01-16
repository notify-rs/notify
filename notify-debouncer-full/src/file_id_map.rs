use crate::FileIdCache;
use file_id::{get_file_id, FileId};
use notify::RecursiveMode;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// A cache to hold the file system IDs of all watched files.
///
/// The file ID cache uses unique file IDs provided by the file system and is used to stitch together
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
    fn cached_file_id(&self, path: &Path) -> Option<impl AsRef<FileId>> {
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
