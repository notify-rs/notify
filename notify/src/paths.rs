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
