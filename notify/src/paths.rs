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

#[derive(Clone, Debug)]
pub(crate) struct WatchMetadata {
    pub(crate) is_recursive: bool,
    pub(crate) reported_path: PathBuf,
    pub(crate) is_user_watch: bool,
    pub(crate) user_is_recursive: bool,
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

/// Names the entries of one recursive walk after the closest recursive user watch above them,
/// so an outer walk does not respell what a nested watch already reports.
///
/// Relies on the walk yielding a directory before its contents.
pub(crate) struct WalkRoots {
    roots: Vec<WatchPath>,
}

impl WalkRoots {
    pub(crate) fn new(root: WatchPath) -> Self {
        Self { roots: vec![root] }
    }

    pub(crate) fn entry(&mut self, path: PathBuf, existing: Option<&WatchMetadata>) -> WatchPath {
        while self.roots.len() > 1 && !path.starts_with(&self.roots[self.roots.len() - 1].absolute)
        {
            self.roots.pop();
        }

        let entry = self.roots[self.roots.len() - 1].child(path);
        if let Some(nested) =
            existing.filter(|watch| watch.is_user_watch && watch.user_is_recursive)
        {
            self.roots.push(WatchPath::from_parts(
                entry.absolute.clone(),
                nested.reported_path.clone(),
            ));
        }
        entry
    }
}

impl WatchMetadata {
    pub(crate) fn new(
        path: &WatchPath,
        is_recursive: bool,
        is_user_watch: bool,
        existing_watch: Option<&Self>,
    ) -> Self {
        let existing_is_user_watch = existing_watch.is_some_and(|watch| watch.is_user_watch);
        let existing_user_is_recursive =
            existing_watch.is_some_and(|watch| watch.user_is_recursive);
        let existing_is_recursive = existing_watch.is_some_and(|watch| watch.is_recursive);

        let reported_path = match existing_watch {
            Some(existing) if !is_user_watch && existing.is_user_watch => {
                existing.reported_path.clone()
            }
            _ => path.requested.clone(),
        };

        Self {
            is_recursive: is_recursive || existing_is_recursive,
            reported_path,
            is_user_watch: is_user_watch || existing_is_user_watch,
            user_is_recursive: if is_user_watch {
                is_recursive
            } else {
                existing_user_is_recursive
            },
        }
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

pub(crate) fn preserved_watch_roots<'a, I>(
    path: &Path,
    remove_recursive: bool,
    watches: I,
) -> Vec<(PathBuf, bool)>
where
    I: IntoIterator<Item = (&'a PathBuf, &'a WatchMetadata)>,
{
    if remove_recursive {
        Vec::new()
    } else {
        watches
            .into_iter()
            .filter(|(candidate, watch)| {
                *candidate != path && candidate.starts_with(path) && watch.is_user_watch
            })
            .map(|(path, watch)| (path.clone(), watch.user_is_recursive))
            .collect()
    }
}

pub(crate) fn is_preserved_watch_root(path: &Path, preserved_roots: &[(PathBuf, bool)]) -> bool {
    preserved_roots.iter().any(|(root, _)| path == root)
}

/// Finds the nearest recursive user watch that covers `path`.
///
/// Backends use this when replacing an explicit watch that also inherits recursive coverage from an
/// ancestor. Returning the ancestor's reported path lets them rebuild the inherited subtree with the
/// same path representation users expect from that ancestor watch.
pub(crate) fn recursive_user_watch_ancestor<'a, I>(
    path: &Path,
    watches: I,
) -> Option<(PathBuf, PathBuf)>
where
    I: IntoIterator<Item = (&'a PathBuf, &'a WatchMetadata)>,
{
    watches
        .into_iter()
        .filter(|(candidate, watch)| {
            *candidate != path
                && path.starts_with(candidate)
                && watch.is_user_watch
                && watch.user_is_recursive
        })
        .max_by_key(|(candidate, _)| candidate.as_os_str().len())
        .map(|(path, watch)| (path.clone(), watch.reported_path.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_watch(reported_path: &str, is_recursive: bool) -> WatchMetadata {
        WatchMetadata {
            is_recursive,
            reported_path: PathBuf::from(reported_path),
            is_user_watch: true,
            user_is_recursive: is_recursive,
        }
    }

    #[test]
    fn walk_roots_name_entries_after_the_closest_recursive_user_watch() {
        let mut roots = WalkRoots::new(WatchPath::from_parts("/repo".into(), "/repo".into()));
        let mut requested = |path: &str, existing: Option<&WatchMetadata>| {
            roots.entry(path.into(), existing).requested
        };

        let logs = user_watch("logs", true);
        let flat = user_watch("flat", false);
        assert_eq!(requested("/repo", None), Path::new("/repo"));
        assert_eq!(
            requested("/repo/logs", Some(&logs)),
            Path::new("/repo/logs")
        );
        assert_eq!(
            requested("/repo/logs/access", None),
            Path::new("logs/access")
        );
        assert_eq!(requested("/repo/src", None), Path::new("/repo/src"));
        assert_eq!(
            requested("/repo/src/flat", Some(&flat)),
            Path::new("/repo/src/flat")
        );
        assert_eq!(
            requested("/repo/src/flat/deep", None),
            Path::new("/repo/src/flat/deep")
        );
    }

    // A directory's create event can arrive after the user has watched that directory by name.
    #[test]
    fn walk_roots_keep_the_spelling_of_a_user_watch_on_the_walk_root() {
        let mut roots = WalkRoots::new(WatchPath::from_parts(
            "/repo/logs".into(),
            "/repo/logs".into(),
        ));
        let logs = user_watch("logs", true);

        roots.entry("/repo/logs".into(), Some(&logs));
        assert_eq!(
            roots.entry("/repo/logs/access".into(), None).requested,
            Path::new("logs/access")
        );
    }
}
