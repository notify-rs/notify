use crate::FileIdCache;
use file_id::{FileId, get_file_id};
use rustc_hash::FxHashMap as HashMap;
use std::path::{Path, PathBuf};

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
    #[must_use]
    pub fn new() -> Self {
        Default::default()
    }
}

impl FileIdCache for FileIdMap {
    fn cached_file_id(&self, path: &Path) -> Option<impl AsRef<FileId>> {
        self.paths.get(path)
    }

    fn add_single_path(&mut self, path: &Path) {
        if let Ok(file_id) = get_file_id(path) {
            self.paths.insert(path.to_path_buf(), file_id);
        }
    }

    fn remove_path(&mut self, path: &Path) {
        self.paths.retain(|p, _| !p.starts_with(path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::RecursiveMode;
    use std::fs;

    #[test]
    fn add_path_walks_as_deep_as_the_recursive_mode() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let child = root.join("child");
        let grandchild = child.join("grandchild");
        fs::create_dir_all(&grandchild).unwrap();

        let mut cache = FileIdMap::new();
        cache.add_path(root, RecursiveMode::NonRecursive);
        assert!(cache.cached_file_id(root).is_some());
        assert!(cache.cached_file_id(&child).is_some());
        assert!(cache.cached_file_id(&grandchild).is_none());

        let mut cache = FileIdMap::new();
        cache.add_path(root, RecursiveMode::Recursive);
        assert!(cache.cached_file_id(&grandchild).is_some());
    }
}
