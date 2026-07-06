use crate::{Error, Result, WatchFilter};
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
    #[allow(dead_code)]
    pub(crate) is_dir: bool,
    pub(crate) is_recursive: bool,
    pub(crate) reported_path: PathBuf,
    pub(crate) is_user_watch: bool,
    pub(crate) user_is_recursive: bool,
    /// The filter governing this entry: the user's requested filter for user watches
    /// (compared on rewatch), and the covering root's filter for entries added by that
    /// root's walks and discovery. The overlap barrier in [`check_watch_barriers`] keeps
    /// filtered watches disjoint, so a single filter per entry is sufficient.
    pub(crate) watch_filter: WatchFilter,
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

impl WatchMetadata {
    pub(crate) fn new<'a, I>(
        path: &WatchPath,
        is_dir: bool,
        is_recursive: bool,
        is_user_watch: bool,
        existing_watch: Option<&Self>,
        _watches: I,
        watch_filter: WatchFilter,
    ) -> Self
    where
        I: IntoIterator<Item = (&'a PathBuf, &'a Self)>,
    {
        let existing_reported_path = existing_watch.map(|watch| watch.reported_path.clone());
        let existing_is_user_watch = existing_watch.is_some_and(|watch| watch.is_user_watch);
        let existing_user_is_recursive =
            existing_watch.is_some_and(|watch| watch.user_is_recursive);
        let existing_is_recursive = existing_watch.is_some_and(|watch| watch.is_recursive);

        let user_is_recursive = if is_user_watch {
            is_recursive
        } else {
            existing_user_is_recursive
        };
        // A user watch records the filter the user passed; merging a walk entry over an
        // existing user watch must not disturb it (the walk's filter is that of a plain
        // overlapping root, which the overlap barrier guarantees is accept-all).
        let watch_filter = if !is_user_watch && existing_is_user_watch {
            existing_watch
                .map(|watch| watch.watch_filter.clone())
                .unwrap_or_else(WatchFilter::accept_all)
        } else {
            watch_filter
        };

        let reported_path = if is_user_watch {
            path.requested.clone()
        } else if existing_is_user_watch {
            existing_reported_path.unwrap_or_else(|| path.requested.clone())
        } else {
            path.requested.clone()
        };

        Self {
            is_recursive: is_recursive || existing_is_recursive,
            is_dir,
            reported_path,
            is_user_watch: is_user_watch || existing_is_user_watch,
            user_is_recursive,
            watch_filter,
        }
    }

    /// Whether a user rewatch with these parameters requests exactly the current watch, so
    /// the backend can skip the teardown/rebuild entirely.
    pub(crate) fn rewatch_is_noop(
        &self,
        path: &WatchPath,
        requested_is_recursive: bool,
        watch_filter: &WatchFilter,
    ) -> bool {
        self.user_is_recursive == requested_is_recursive
            && self.reported_path == path.requested
            && self.watch_filter.same_filter(watch_filter)
    }
}

/// Enforces both `watch_filtered` barriers for a user watch request, before any backend
/// state is touched: a directory root the filter itself rejects (checking a symlink root's
/// target too) is refused with [`crate::ErrorKind::PathExcluded`], and a directory watch
/// involving a filter may not overlap another user directory watch in either direction,
/// since filters are never merged across watches. Accept-all watches keep the pre-filter
/// overlap semantics; file watches never conflict.
pub(crate) fn check_watch_barriers<'a, I>(
    absolute: &Path,
    requested: &Path,
    is_dir: bool,
    is_recursive: bool,
    watch_filter: &WatchFilter,
    user_watches: I,
) -> Result<()>
where
    I: IntoIterator<Item = (&'a PathBuf, bool, bool, &'a WatchFilter)>,
{
    if root_rejected(watch_filter, absolute, is_dir) {
        return Err(Error::path_excluded().add_path(requested.to_path_buf()));
    }
    if let Some(conflicting) =
        filtered_watch_conflict(absolute, is_dir, is_recursive, watch_filter, user_watches)
    {
        return Err(
            Error::generic("a watch with a filter must not overlap another watch")
                .add_path(requested.to_path_buf())
                .add_path(conflicting),
        );
    }
    Ok(())
}

/// Returns the path of an existing user watch the new watch may not coexist with, if any.
/// See [`check_watch_barriers`] for the contract.
fn filtered_watch_conflict<'a, I>(
    path: &Path,
    path_is_dir: bool,
    is_recursive: bool,
    watch_filter: &WatchFilter,
    user_watches: I,
) -> Option<PathBuf>
where
    I: IntoIterator<Item = (&'a PathBuf, bool, bool, &'a WatchFilter)>,
{
    if !path_is_dir {
        return None;
    }
    let canonical_path = std::fs::canonicalize(path).ok();
    for (candidate, candidate_is_dir, candidate_is_recursive, candidate_filter) in user_watches {
        if !candidate_is_dir {
            continue;
        }
        if candidate.as_path() == path {
            // Rewatching the same path replaces the watch.
            continue;
        }
        if watch_filter.is_accept_all() && candidate_filter.is_accept_all() {
            continue;
        }
        let canonical_candidate = std::fs::canonicalize(candidate).ok();
        let covers_new = candidate_is_recursive
            && any_starts_with(
                path,
                canonical_path.as_deref(),
                candidate,
                canonical_candidate.as_deref(),
            );
        // The new watch only covers existing DIRECTORY watches; a watched file inside the
        // new subtree does not interact with directory filtering.
        let new_covers = is_recursive
            && any_starts_with(
                candidate,
                canonical_candidate.as_deref(),
                path,
                canonical_path.as_deref(),
            );
        if covers_new || new_covers {
            return Some(candidate.clone());
        }
    }
    None
}

fn any_starts_with(
    path: &Path,
    canonical_path: Option<&Path>,
    prefix: &Path,
    canonical_prefix: Option<&Path>,
) -> bool {
    path.starts_with(prefix)
        || canonical_path.is_some_and(|path| path.starts_with(prefix))
        || canonical_prefix.is_some_and(|prefix| path.starts_with(prefix))
        || canonical_path
            .zip(canonical_prefix)
            .is_some_and(|(path, prefix)| path.starts_with(prefix))
}

/// Root-level filter gate shared by the backends: whether the directory root at `absolute`
/// is rejected by `filter`. A symlink root is checked against its resolved target too, so
/// an excluded directory cannot be watched through a link. Non-directories are never
/// rejected: the filter gates directories only.
fn root_rejected(filter: &WatchFilter, absolute: &Path, is_dir: bool) -> bool {
    if !is_dir || filter.is_accept_all() {
        return false;
    }
    let is_symlink = std::fs::symlink_metadata(absolute)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false);
    !filter_allows_dir(filter, absolute, is_symlink)
}

/// Whether the filter allows watching the directory at `path`. A symlink to a directory is
/// checked against both its own path and its resolved target, so an excluded directory
/// cannot be watched through a link.
pub(crate) fn filter_allows_dir(filter: &WatchFilter, path: &Path, is_symlink: bool) -> bool {
    if filter.is_accept_all() {
        // Skip the filter and (for symlinks) the canonicalize syscall entirely: unfiltered
        // watches must not pay filtering costs.
        return true;
    }
    if !filter.should_watch(path) {
        return false;
    }
    if is_symlink {
        // The caller determined the target is a directory, but `path` is the link side;
        // consult the target too so name-based exclusions cannot be bypassed via symlinks.
        if let Ok(target) = std::fs::canonicalize(path) {
            return filter.should_watch(&target);
        }
    }
    true
}

/// Shared `WalkDir::filter_entry` predicate implementing the `WatchFilter` pruning contract:
/// the filter gates directories only (files always pass), and a rejected directory is neither
/// yielded nor descended into.
pub(crate) fn filter_keeps_walk_entry(filter: &WatchFilter, entry: &walkdir::DirEntry) -> bool {
    !entry.file_type().is_dir() || filter_allows_dir(filter, entry.path(), entry.path_is_symlink())
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

/// Finds the nearest recursive user watch covering `path`, returning its path and reported
/// path.
///
/// Backends use this when replacing an explicit watch that also inherits recursive coverage
/// from an ancestor, so the rebuilt subtree keeps the path representation users expect from
/// that watch. The overlap barrier guarantees any such ancestor is unfiltered (a filtered
/// watch never overlaps another watch), so the rebuild needs no filter gating.
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
            candidate.as_path() != path
                && path.starts_with(candidate)
                && watch.is_user_watch
                && watch.user_is_recursive
        })
        .max_by_key(|(candidate, _)| candidate.as_os_str().len())
        .map(|(candidate, watch)| (candidate.clone(), watch.reported_path.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorKind;
    use std::ffi::OsStr;

    fn reject_name(name: &'static str) -> WatchFilter {
        WatchFilter::with_filter(move |p: &Path| p.file_name() != Some(OsStr::new(name)))
    }

    fn check(
        absolute: &Path,
        is_dir: bool,
        is_recursive: bool,
        watch_filter: &WatchFilter,
        user_watches: &[(PathBuf, bool, bool, WatchFilter)],
    ) -> Result<()> {
        check_watch_barriers(
            absolute,
            absolute,
            is_dir,
            is_recursive,
            watch_filter,
            user_watches
                .iter()
                .map(|(path, is_dir, is_recursive, filter)| (path, *is_dir, *is_recursive, filter)),
        )
    }

    #[test]
    fn check_watch_barriers_rejects_filtered_directory_root() {
        let dir = tempfile::tempdir().unwrap();
        let filter = WatchFilter::with_filter({
            let root = dir.path().to_path_buf();
            move |p: &Path| p != root.as_path()
        });

        let result = check(dir.path(), true, true, &filter, &[]);

        assert!(
            matches!(&result, Err(error) if matches!(error.kind, ErrorKind::PathExcluded)),
            "watching a rejected directory root must fail with PathExcluded: {result:?}"
        );
        assert_eq!(result.unwrap_err().paths, vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn check_watch_barriers_does_not_reject_file_roots() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("watched.txt");
        std::fs::write(&file, "data").unwrap();
        let filter = WatchFilter::with_filter({
            let file = file.clone();
            move |p: &Path| p != file.as_path()
        });

        check(&file, false, false, &filter, &[]).expect("file roots are never filtered");
    }

    #[test]
    fn check_watch_barriers_allows_accept_all_overlaps_and_same_path_rewatch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let child = root.join("child");
        std::fs::create_dir(&child).unwrap();
        let accept_all = WatchFilter::accept_all();
        let filtered = reject_name("excluded");

        let accept_all_watch = vec![(root.clone(), true, true, WatchFilter::accept_all())];
        check(&child, true, true, &accept_all, &accept_all_watch)
            .expect("accept-all directory watches may overlap");

        let same_path_watch = vec![(root.clone(), true, true, filtered.clone())];
        check(&root, true, true, &filtered, &same_path_watch)
            .expect("rewatching the same path is replacement, not overlap");
    }

    #[test]
    fn check_watch_barriers_rejects_filtered_overlap_in_both_directions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let child = root.join("child");
        std::fs::create_dir(&child).unwrap();
        let grandchild = child.join("grandchild");
        std::fs::create_dir(&grandchild).unwrap();
        let filter = reject_name("excluded");

        let recursive_root = vec![(root.clone(), true, true, WatchFilter::accept_all())];
        assert!(
            check(&child, true, true, &filter, &recursive_root).is_err(),
            "a filtered watch nested under a recursive directory watch must be refused"
        );

        let filtered_root = vec![(root.clone(), true, true, filter.clone())];
        assert!(
            check(
                &child,
                true,
                false,
                &WatchFilter::accept_all(),
                &filtered_root,
            )
            .is_err(),
            "a directory watch inside a filtered recursive watch must be refused"
        );

        let nested_directory = vec![(grandchild.clone(), true, false, WatchFilter::accept_all())];
        assert!(
            check(&child, true, true, &filter, &nested_directory).is_err(),
            "a filtered recursive watch over an existing directory watch must be refused"
        );
    }

    #[test]
    fn check_watch_barriers_ignores_file_watch_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let file = root.join("file.txt");
        std::fs::write(&file, "data").unwrap();
        let file_watch = vec![(file, false, false, WatchFilter::accept_all())];

        check(&root, true, true, &reject_name("excluded"), &file_watch)
            .expect("file watches never conflict with filtered directory watches");
    }

    #[cfg(unix)]
    #[test]
    fn check_watch_barriers_detects_symlink_alias_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let alias = dir.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let real_watch = vec![(real, true, true, WatchFilter::accept_all())];

        assert!(
            check(&alias, true, true, &reject_name("excluded"), &real_watch).is_err(),
            "canonicalized aliases must not bypass filtered overlap checks"
        );
    }

    #[cfg(unix)]
    #[test]
    fn filter_allows_dir_checks_symlink_target() {
        let dir = tempfile::tempdir().unwrap();
        let excluded = dir.path().join("excluded");
        let alias = dir.path().join("alias");
        std::fs::create_dir(&excluded).unwrap();
        std::os::unix::fs::symlink(&excluded, &alias).unwrap();

        assert!(
            !filter_allows_dir(&reject_name("excluded"), &alias, true),
            "a symlink to an excluded directory must be rejected"
        );
    }
}
