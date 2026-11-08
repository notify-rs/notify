use crate::FileIdCache;
use file_id::{FileId, get_file_id};
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    path::{Component, Path},
    sync::Arc,
};

/// A cache to hold the file system IDs of all watched files.
///
/// The file ID cache uses unique file IDs provided by the file system and is used to stitch together
/// rename events in case the notification back-end doesn't emit rename cookies.
#[derive(Debug, Clone, Default)]
pub struct FileIdMap {
    paths: FxHashMap<Arc<Path>, FileId>,
    // Index direct children, including ancestors that have no cached ID, so removing
    // a subtree never needs to scan unrelated paths. Share keys with the ID map.
    children: FxHashMap<Arc<Path>, FxHashSet<Arc<Path>>>,
}

impl FileIdMap {
    /// Construct an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Default::default()
    }

    fn insert(&mut self, path: &Path, file_id: FileId) {
        if let Some(cached) = self.paths.get_mut(path) {
            *cached = file_id;
        } else {
            let path: Arc<Path> = path.into();
            self.paths.insert(Arc::clone(&path), file_id);
            self.index_path(path);
        }
    }

    fn index_path(&mut self, mut path: Arc<Path>) {
        while let Some(parent) = parent_path(&path) {
            if let Some(children) = self.children.get_mut(parent) {
                children.insert(path);
                return;
            }
            let parent = self
                .paths
                .get_key_value(parent)
                .map_or_else(|| Arc::<Path>::from(parent), |(path, _)| Arc::clone(path));
            self.children
                .insert(Arc::clone(&parent), FxHashSet::from_iter([path]));
            path = parent;
        }
    }

    // Unlink the subtree and prune uncached ancestors with no remaining children.
    // Reaching the empty prefix means that no cached entries exist outside it.
    fn unlink_path(&mut self, mut path: &Path) -> bool {
        while let Some(parent) = parent_path(path) {
            let Some(children) = self.children.get_mut(parent) else {
                return false;
            };
            children.remove(path);
            if !children.is_empty() {
                return false;
            }
            self.children.remove(parent);
            if self.paths.contains_key(parent) {
                return false;
            }
            path = parent;
        }
        true
    }
}

impl FileIdCache for FileIdMap {
    fn cached_file_id(&self, path: &Path) -> Option<impl AsRef<FileId>> {
        self.paths.get(path)
    }

    fn add_single_path(&mut self, path: &Path) {
        if let Ok(file_id) = get_file_id(path) {
            self.insert(path, file_id);
        }
    }

    fn remove_path(&mut self, path: &Path) {
        if self.unlink_path(path) {
            self.paths.clear();
            self.children.clear();
            return;
        }
        self.paths.remove(path);
        let mut pending: Vec<_> = self.children.remove(path).into_iter().flatten().collect();
        while let Some(path) = pending.pop() {
            self.paths.remove(path.as_ref());
            if let Some(children) = self.children.remove(path.as_ref()) {
                pending.extend(children);
            }
        }
    }
}

// Unlike Path::parent, this also links roots and Windows prefixes to the empty
// prefix, matching the component-prefix semantics of Path::starts_with.
fn parent_path(path: &Path) -> Option<&Path> {
    let mut components = path.components();
    Some(match components.next_back()? {
        Component::Prefix(_) => Path::new(""),
        // UNC prefixes have an implicit root, so removing their RootDir may
        // yield the same path. Link those directly to the empty prefix.
        Component::RootDir if components.as_path() == path => Path::new(""),
        _ => components.as_path(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::RecursiveMode;
    use std::fs;

    fn id(inode_number: u64) -> FileId {
        FileId::Inode {
            device_id: 1,
            inode_number,
        }
    }

    #[test]
    fn removals_match_component_prefixes() {
        let mut paths = vec![
            "a/b/file",
            "a/b/nested/leaf",
            "a/bc/file",
            "a/other",
            "other/file",
            "./a/b/file",
            "/absolute/a/file",
            "/absolute/b/file",
        ];
        let mut prefixes = vec![
            "",
            ".",
            "a",
            "a/b",
            "a//./b",
            "a/b/",
            "a/b/file",
            "a/absent",
            "absent",
            "./a",
            "other",
            "/",
            "/absolute/a",
        ];
        if cfg!(windows) {
            paths.extend([
                r"C:\a\file",
                r"C:\b\file",
                r"C:a\file",
                r"D:\a\file",
                r"\\server\share\a\file",
                r"\\server\other\file",
                r"\\?\C:\a\file",
                r"\\?\UNC\server\share\a\file",
                r"\\.\device\a\file",
            ]);
            prefixes.extend([
                "C:",
                r"C:\",
                r"C:\a",
                r"C:a",
                r"D:\",
                r"\\server\share",
                r"\\server\share\",
                r"\\server\share\a",
                r"\\?\C:",
                r"\\?\C:\",
                r"\\?\UNC\server\share",
                r"\\?\UNC\server\share\",
                r"\\.\device",
            ]);
        }
        for prefix in prefixes {
            let mut cache = FileIdMap::new();
            for (i, path) in paths.iter().enumerate() {
                cache.insert(Path::new(path), id(i as u64));
            }
            cache.remove_path(Path::new(prefix));
            for (i, path) in paths.iter().enumerate() {
                let expected = (!Path::new(path).starts_with(prefix)).then(|| id(i as u64));
                assert_eq!(
                    cache.cached_file_id(Path::new(path)).map(|id| *id.as_ref()),
                    expected,
                    "removing {prefix:?}, checking {path:?}"
                );
            }
        }
    }

    #[test]
    fn mixed_insertions_and_removals_match_a_flat_cache() {
        let paths = [
            "root",
            "root/a",
            "root/a/file",
            "root/a/nested/file",
            "root/ab/file",
            "other/file",
            "./relative/file",
            "/absolute/file",
        ];
        let prefixes = ["root/a", "other", "missing", "root", "./relative", "/", ""];
        let mut cache = FileIdMap::new();
        let mut expected = FxHashMap::default();
        for step in 0..128 {
            if step % 3 != 0 {
                let path = Path::new(paths[step * 7 % paths.len()]);
                let file_id = id(step as u64);
                cache.insert(path, file_id);
                expected.insert(path, file_id);
            } else {
                let prefix = Path::new(prefixes[step / 3 % prefixes.len()]);
                cache.remove_path(prefix);
                expected.retain(|path, _| !path.starts_with(prefix));
            }
            for path in paths {
                assert_eq!(
                    cache.cached_file_id(Path::new(path)).map(|id| *id.as_ref()),
                    expected.get(Path::new(path)).copied(),
                    "step {step}, path {path}"
                );
            }
        }
        cache.remove_path(Path::new(""));
        assert!(cache.paths.is_empty());
        assert!(cache.children.is_empty());
    }

    #[test]
    fn removing_children_preserves_a_later_cached_parent() {
        let parent = Path::new("root/parent");
        let child = Path::new("root/parent/child");
        let mut cache = FileIdMap::new();
        cache.insert(child, id(1));
        cache.insert(parent, id(2));
        let cloned = cache.clone();
        cache.insert(parent, id(3));
        cache.remove_path(child);
        assert!(cache.cached_file_id(child).is_none());
        assert_eq!(*cache.cached_file_id(parent).unwrap().as_ref(), id(3));
        assert_eq!(*cloned.cached_file_id(parent).unwrap().as_ref(), id(2));
        assert_eq!(*cloned.cached_file_id(child).unwrap().as_ref(), id(1));

        cache.insert(child, id(4));
        cache.remove_path(parent);
        assert!(cache.paths.is_empty());
        assert!(cache.children.is_empty());
    }

    #[test]
    fn equivalent_spellings_update_one_entry_and_can_be_removed() {
        let mut cache = FileIdMap::new();
        cache.insert(Path::new("a/b/file"), id(1));
        cache.insert(Path::new("a//./b//file"), id(2));
        cache.insert(Path::new("a/sibling"), id(3));
        assert_eq!(
            *cache
                .cached_file_id(Path::new("a/b/file"))
                .unwrap()
                .as_ref(),
            id(2)
        );
        cache.remove_path(Path::new("a//./b"));
        assert!(cache.cached_file_id(Path::new("a/b/file")).is_none());
        assert_eq!(
            *cache
                .cached_file_id(Path::new("a/sibling"))
                .unwrap()
                .as_ref(),
            id(3)
        );
        cache.remove_path(Path::new("a"));
        assert!(cache.paths.is_empty());
        assert!(cache.children.is_empty());
    }

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

    #[test]
    fn remove_subtree_without_cached_root_keeps_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let removed = dir.path().join("a/b");
        let child = removed.join("child");
        let descendant = removed.join("nested/leaf");
        let sibling = dir.path().join("a/bc/child");
        let earlier = dir.path().join("a/a");
        for path in [&child, &descendant, &sibling, &earlier] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"file").unwrap();
        }
        let mut cache = FileIdMap::new();
        for path in [&child, &descendant, &sibling, &earlier] {
            cache.add_single_path(path);
        }
        assert!(cache.cached_file_id(&removed).is_none());

        // A missing prefix must leave unrelated entries intact.
        cache.remove_path(&dir.path().join("a/aa"));
        assert!(cache.cached_file_id(&child).is_some());
        cache.remove_path(&removed);
        assert!(cache.cached_file_id(&child).is_none());
        assert!(cache.cached_file_id(&descendant).is_none());
        assert!(cache.cached_file_id(&sibling).is_some());
        assert!(cache.cached_file_id(&earlier).is_some());

        cache.add_path(&removed, RecursiveMode::Recursive);
        assert!(cache.cached_file_id(&descendant).is_some());
        cache.remove_path(dir.path());
        for path in [&removed, &child, &descendant, &sibling, &earlier] {
            assert!(cache.cached_file_id(path).is_none());
        }
    }

    #[test]
    fn updating_a_file_id_and_removing_it_leave_a_clone_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        fs::write(&path, b"old").unwrap();
        let mut cache = FileIdMap::new();
        cache.add_single_path(&path);
        let old_id = *cache.cached_file_id(&path).unwrap().as_ref();
        let cloned = cache.clone();

        // Keep the old inode alive so the replacement must have a different ID.
        fs::rename(&path, dir.path().join("old")).unwrap();
        fs::write(&path, b"new").unwrap();
        let new_id = get_file_id(&path).unwrap();
        assert_ne!(old_id, new_id);
        cache.add_single_path(&path);
        cache.add_single_path(&path);
        assert_eq!(*cache.cached_file_id(&path).unwrap().as_ref(), new_id);

        cache.remove_path(&path);
        assert!(cache.cached_file_id(&path).is_none());
        assert_eq!(*cloned.cached_file_id(&path).unwrap().as_ref(), old_id);
        cache.add_single_path(&path);
        assert_eq!(*cache.cached_file_id(&path).unwrap().as_ref(), new_id);
    }

    #[test]
    fn equivalent_path_spellings_use_the_same_cache_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/file");
        fs::create_dir(path.parent().unwrap()).unwrap();
        fs::write(&path, b"file").unwrap();
        let mut cache = FileIdMap::new();
        cache.add_single_path(&path);
        let equivalent = dir.path().join("sub//./file");
        assert!(cache.cached_file_id(&equivalent).is_some());
        cache.remove_path(&equivalent);
        assert!(cache.cached_file_id(&path).is_none());
    }
}
