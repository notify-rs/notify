use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use file_id::{get_file_id, FileId};
use notify::RecursiveMode;
use walkdir::WalkDir;

pub trait FileIdCache {
    fn cached_file_id(&self, path: &Path) -> Option<&FileId>;

    fn add_path(&mut self, path: &Path);

    fn remove_path(&mut self, path: &Path);

    fn rescan(&mut self);
}

#[derive(Debug, Clone, Default)]
pub struct PathCache {
    paths: HashMap<PathBuf, FileId>,
    roots: Vec<(PathBuf, RecursiveMode)>,
}

impl PathCache {
    pub fn add_root(&mut self, path: impl Into<PathBuf>, recursive_mode: RecursiveMode) {
        let path = path.into();

        self.roots.push((path.clone(), recursive_mode));

        self.add_path(&path);
    }

    pub fn remove_root(&mut self, path: impl AsRef<Path>) {
        self.roots.retain(|(root, _)| !root.starts_with(&path));

        self.remove_path(path.as_ref());
    }

    fn dir_scan_depth(is_recursive: bool) -> usize {
        if is_recursive {
            usize::max_value()
        } else {
            1
        }
    }
}

impl FileIdCache for PathCache {
    fn cached_file_id(&self, path: &Path) -> Option<&FileId> {
        self.paths.get(path)
    }

    fn add_path(&mut self, path: &Path) {
        let is_recursive = self
            .roots
            .iter()
            .find_map(|(root, recursive_mode)| {
                if path.starts_with(root) {
                    Some(*recursive_mode == RecursiveMode::Recursive)
                } else {
                    None
                }
            })
            .unwrap_or_default();

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

    fn rescan(&mut self) {
        for (root, _) in self.roots.clone() {
            self.add_path(&root);
        }
    }
}

pub struct NoCache;

impl FileIdCache for NoCache {
    fn cached_file_id(&self, _path: &Path) -> Option<&FileId> {
        None
    }

    fn add_path(&mut self, _path: &Path) {}

    fn remove_path(&mut self, _path: &Path) {}

    fn rescan(&mut self) {}
}
