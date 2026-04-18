use crate::{Error, Result};
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub(crate) struct WatchPath {
    pub(crate) absolute: PathBuf,
    pub(crate) requested: PathBuf,
}

impl WatchPath {
    pub(crate) fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            absolute: absolute_path(path)?,
            requested: path.to_path_buf(),
        })
    }

    pub(crate) fn from_parts(absolute: PathBuf, requested: PathBuf) -> Self {
        Self {
            absolute,
            requested,
        }
    }

    pub(crate) fn child(&self, path: PathBuf) -> Self {
        let requested = reported_path(&self.absolute, &self.requested, &path);
        Self::from_parts(path, requested)
    }
}

pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir().map_err(Error::io)?.join(path))
    }
}

pub(crate) fn reported_path(root_absolute: &Path, root_requested: &Path, path: &Path) -> PathBuf {
    debug_assert!(
        path.starts_with(root_absolute),
        "reported_path called with path outside root: root={}, path={}",
        root_absolute.display(),
        path.display()
    );

    match path.strip_prefix(root_absolute) {
        Ok(relative) if !relative.as_os_str().is_empty() => root_requested.join(relative),
        _ => root_requested.to_path_buf(),
    }
}

pub(crate) fn preserved_watch_mode(
    path: &Path,
    preserved_roots: &[(PathBuf, bool)],
) -> Option<bool> {
    preserved_roots
        .iter()
        .find(|(root, user_is_recursive)| {
            path == root || (*user_is_recursive && path.starts_with(root))
        })
        .map(|(_, user_is_recursive)| *user_is_recursive)
}

pub(crate) fn is_preserved_watch_root(path: &Path, preserved_roots: &[(PathBuf, bool)]) -> bool {
    preserved_roots.iter().any(|(root, _)| path == root)
}
