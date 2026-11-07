use crate::ErrorKind;

/// What a failed watch installation means for the rest of a recursive walk.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WalkFailure {
    /// Nothing was lost, so the walk carries on.
    Skip,
    /// This path errored, but the rest of the tree will still be walked.
    Report,
    /// The watch limit prevents further additions, so the walk ends here.
    Abort,
}

impl WalkFailure {
    pub(crate) fn of(kind: &ErrorKind) -> Self {
        match kind {
            // TOCTOU: an entry can disappear between being listed and being watched.
            ErrorKind::PathNotFound => Self::Skip,
            ErrorKind::MaxFilesWatch => Self::Abort,
            _ => Self::Report,
        }
    }
}

// Inotify and kqueue manage recursive watches one directory at a time.
pub(crate) mod recursive {
    use crate::paths::{WatchPath, reported_path};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    #[derive(Clone, Debug)]
    pub(crate) struct WatchMetadata {
        pub(crate) is_recursive: bool,
        pub(crate) reported_path: PathBuf,
        pub(crate) is_user_watch: bool,
        pub(crate) user_is_recursive: bool,
        pub(crate) user_data: Option<u64>,
    }

    impl WatchPath {
        pub(crate) fn from_parts(
            absolute: PathBuf,
            requested: PathBuf,
            user_data: Option<u64>,
        ) -> Self {
            Self {
                absolute,
                requested,
                user_data,
            }
        }

        pub(crate) fn child(&self, path: PathBuf) -> Self {
            let requested = reported_path(&self.absolute, &self.requested, &path);
            Self::from_parts(path, requested, self.user_data)
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

        pub(crate) fn entry(
            &mut self,
            path: PathBuf,
            existing: Option<&WatchMetadata>,
        ) -> WatchPath {
            while self.roots.len() > 1
                && !path.starts_with(&self.roots[self.roots.len() - 1].absolute)
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
                    nested.user_data,
                ));
            }
            entry
        }
    }

    impl WatchMetadata {
        pub(crate) fn user_data_for(
            path: &WatchPath,
            is_user_watch: bool,
            existing_watch: Option<&Self>,
        ) -> Option<u64> {
            match existing_watch {
                Some(existing) if !is_user_watch && existing.is_user_watch => existing.user_data,
                _ => path.user_data,
            }
        }

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
            let user_data = Self::user_data_for(path, is_user_watch, existing_watch);

            Self {
                is_recursive: is_recursive || existing_is_recursive,
                reported_path,
                is_user_watch: is_user_watch || existing_is_user_watch,
                user_is_recursive: if is_user_watch {
                    is_recursive
                } else {
                    existing_user_is_recursive
                },
                user_data,
            }
        }
    }

    pub(crate) fn preserved_watch_mode(
        path: &Path,
        preserved_roots: &HashMap<&Path, bool>,
    ) -> Option<bool> {
        if preserved_roots.is_empty() {
            return None;
        }
        // Non-recursive roots own only themselves; keep looking above them for coverage.
        path.ancestors().find_map(|ancestor| {
            let is_recursive = *preserved_roots.get(ancestor)?;
            (ancestor == path || is_recursive).then_some(is_recursive)
        })
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
                    watch.is_user_watch && *candidate != path && candidate.starts_with(path)
                })
                .map(|(path, watch)| (path.clone(), watch.user_is_recursive))
                .collect()
        }
    }

    /// Selects descendant watches to remove or reset after the root watch was removed.
    /// Only scan unrelated watches once; nested user watches are found in the selected subtree.
    pub(crate) fn watch_removal_paths<'a>(
        path: &Path,
        remove_recursive: bool,
        watches: impl Iterator<Item = (&'a PathBuf, &'a WatchMetadata)>,
    ) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let descendants: Vec<_> = watches
            .filter(|(candidate, _)| candidate.starts_with(path))
            .collect();
        let preserved_roots = descendants
            .iter()
            .filter(|(_, watch)| !remove_recursive && watch.is_user_watch)
            .map(|(path, watch)| (path.as_path(), watch.user_is_recursive))
            .collect::<HashMap<_, _>>();
        let mut remove = Vec::new();
        let mut reset = Vec::new();
        for (candidate, _) in descendants {
            if preserved_roots.contains_key(candidate.as_path()) {
                reset.push(candidate.clone());
            } else if preserved_watch_mode(candidate, &preserved_roots).is_none() {
                remove.push(candidate.clone());
            }
        }
        (remove, reset)
    }

    /// Finds the nearest recursive user watch strictly above `path`.
    ///
    /// Used for dynamic additions and for rebuilding inherited coverage when replacing a watch.
    /// Look up ancestors by path so the cost depends on depth, not the number of watches.
    pub(crate) fn recursive_user_watch_ancestor<'a>(
        path: &Path,
        mut watch_for: impl FnMut(&Path) -> Option<&'a WatchMetadata>,
    ) -> Option<WatchPath> {
        path.ancestors().skip(1).find_map(|ancestor| {
            let watch = watch_for(ancestor)?;
            (watch.is_user_watch && watch.user_is_recursive).then(|| {
                WatchPath::from_parts(
                    ancestor.to_path_buf(),
                    watch.reported_path.clone(),
                    watch.user_data,
                )
            })
        })
    }

    /// Updates a user watch and its inherited descendants without changing OS watches.
    pub(crate) fn update_watch_user_data<'a>(
        root: &Path,
        is_recursive: bool,
        user_data: Option<u64>,
        preserved_roots: &[(PathBuf, bool)],
        watches: impl Iterator<Item = (&'a PathBuf, &'a mut WatchMetadata)>,
    ) {
        // The owned snapshot allows the caller to mutate the watch map during this pass.
        let preserved_roots = preserved_roots
            .iter()
            .map(|(path, recursive)| (path.as_path(), *recursive))
            .collect();
        for (path, watch) in watches {
            if path == root
                || (is_recursive
                    && path.starts_with(root)
                    && preserved_watch_mode(path, &preserved_roots).is_none())
            {
                watch.user_data = user_data;
            }
        }
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
                user_data: None,
            }
        }

        #[test]
        fn preserved_modes_skip_non_recursive_ancestors() {
            let roots = HashMap::from([
                (Path::new("/root/recursive"), true),
                (Path::new("/root/recursive/flat"), false),
                (Path::new("/root/flat"), false),
            ]);
            for (path, expected) in [
                ("/root/recursive", Some(true)),
                ("/root/recursive/flat", Some(false)),
                ("/root/recursive/flat/child", Some(true)),
                ("/root/flat", Some(false)),
                ("/root/flat/child", None),
                ("/root/recursive-other", None),
                ("/root", None),
            ] {
                assert_eq!(preserved_watch_mode(Path::new(path), &roots), expected);
            }
            assert_eq!(
                preserved_watch_mode(Path::new("/root/recursive"), &HashMap::new()),
                None
            );
        }

        #[test]
        fn removal_preserves_nested_user_watches_and_their_recursive_children() {
            use std::collections::HashMap;
            let mut inherited = user_watch("inherited", true);
            inherited.is_user_watch = false;
            inherited.user_is_recursive = false;
            let mut flat = user_watch("flat", false);
            flat.is_recursive = true;
            // The caller has already removed /root itself.
            let watches = HashMap::from([
                (PathBuf::from("/root/inherited"), inherited.clone()),
                (PathBuf::from("/root/flat"), flat.clone()),
                (PathBuf::from("/root/flat/inherited"), inherited.clone()),
                (
                    PathBuf::from("/root/flat/nested"),
                    user_watch("nested", true),
                ),
                (PathBuf::from("/root/flat/nested/child"), inherited.clone()),
                (
                    PathBuf::from("/root/recursive"),
                    user_watch("recursive", true),
                ),
                (PathBuf::from("/root/recursive/child"), inherited.clone()),
                (PathBuf::from("/root/recursive/flat"), flat),
                (
                    PathBuf::from("/root/recursive/flat/child"),
                    inherited.clone(),
                ),
                (PathBuf::from("/root-other/child"), inherited),
            ]);
            let (mut remove, mut reset) =
                watch_removal_paths(Path::new("/root"), false, watches.iter());
            remove.sort();
            reset.sort();
            assert_eq!(
                remove,
                ["/root/flat/inherited", "/root/inherited"].map(PathBuf::from)
            );
            assert_eq!(
                reset,
                [
                    "/root/flat",
                    "/root/flat/nested",
                    "/root/recursive",
                    "/root/recursive/flat",
                ]
                .map(PathBuf::from)
            );

            let (mut remove, reset) = watch_removal_paths(Path::new("/root"), true, watches.iter());
            remove.sort();
            assert!(reset.is_empty());
            assert_eq!(
                remove,
                [
                    "/root/flat",
                    "/root/flat/inherited",
                    "/root/flat/nested",
                    "/root/flat/nested/child",
                    "/root/inherited",
                    "/root/recursive",
                    "/root/recursive/child",
                    "/root/recursive/flat",
                    "/root/recursive/flat/child",
                ]
                .map(PathBuf::from)
            );
        }

        #[test]
        fn user_data_ancestor_lookup_skips_inherited_and_non_recursive_watches() {
            use std::collections::HashMap;
            let mut root = user_watch("root", true);
            root.user_data = Some(1);
            let mut flat = user_watch("flat", false);
            flat.is_recursive = true;
            flat.user_data = Some(2);
            let mut inherited = root.clone();
            inherited.is_user_watch = false;
            inherited.user_is_recursive = false;
            let mut watches = HashMap::from([
                (PathBuf::from("/root"), root),
                (PathBuf::from("/root/flat"), flat),
                (PathBuf::from("/root/flat/deep"), inherited),
            ]);
            for path in ["/root/new", "/root/flat/new", "/root/flat/deep/new"] {
                let owner =
                    recursive_user_watch_ancestor(Path::new(path), |p| watches.get(p)).unwrap();
                assert_eq!(owner.absolute, Path::new("/root"));
                assert_eq!(owner.user_data, Some(1));
            }
            // A recursive user watch with no value still owns its descendants.
            watches.insert("/root/flat/deep".into(), user_watch("deep", true));
            let owner =
                recursive_user_watch_ancestor(Path::new("/root/flat/deep/new"), |p| watches.get(p))
                    .unwrap();
            assert_eq!(owner.absolute, Path::new("/root/flat/deep"));
            assert_eq!(owner.user_data, None);
            // Replacement looks above the watch being replaced, excluding the watch itself.
            assert_eq!(
                recursive_user_watch_ancestor(Path::new("/root/flat/deep"), |p| watches.get(p))
                    .unwrap()
                    .user_data,
                Some(1)
            );
        }

        #[test]
        fn user_data_updates_only_owned_descendants() {
            use std::collections::HashMap;
            let root = PathBuf::from("/root");
            let mut inherited = user_watch("inherited", true);
            inherited.is_user_watch = false;
            inherited.user_is_recursive = false;
            let mut watches = HashMap::from([
                (root.clone(), user_watch("root", true)),
                (root.join("flat"), user_watch("flat", false)),
                (root.join("flat/inherited"), inherited.clone()),
                (root.join("nested"), user_watch("nested", true)),
                (root.join("nested/inherited"), inherited.clone()),
                (root.join("nested/flat"), user_watch("nested/flat", false)),
                (root.join("nested/flat/inherited"), inherited),
            ]);
            let protected = preserved_watch_roots(&root, false, watches.iter());
            update_watch_user_data(&root, true, Some(42), &protected, watches.iter_mut());
            assert_eq!(watches[&root].user_data, Some(42));
            assert_eq!(watches[&root.join("flat/inherited")].user_data, Some(42));
            for path in [
                "flat",
                "nested",
                "nested/inherited",
                "nested/flat",
                "nested/flat/inherited",
            ] {
                assert_eq!(watches[&root.join(path)].user_data, None);
            }
            update_watch_user_data(&root.join("flat"), false, Some(7), &[], watches.iter_mut());
            assert_eq!(watches[&root.join("flat")].user_data, Some(7));
            assert_eq!(watches[&root.join("flat/inherited")].user_data, Some(42));
            update_watch_user_data(&root, true, None, &protected, watches.iter_mut());
            assert_eq!(watches[&root.join("flat")].user_data, Some(7));
            assert_eq!(watches[&root.join("flat/inherited")].user_data, None);
        }

        #[test]
        fn walk_roots_preserve_nested_user_data_including_none() {
            let root = WatchPath::from_parts("/root".into(), "root".into(), Some(1));
            let mut roots = WalkRoots::new(root);
            let mut nested = user_watch("nested", true);
            nested.user_data = Some(2);
            roots.entry("/root".into(), None);
            roots.entry("/root/nested".into(), Some(&nested));
            assert_eq!(
                roots.entry("/root/nested/child".into(), None).user_data,
                Some(2)
            );
            roots.entry("/root/empty".into(), Some(&user_watch("empty", true)));
            assert_eq!(
                roots.entry("/root/empty/child".into(), None).user_data,
                None
            );
            assert_eq!(roots.entry("/root/sibling".into(), None).user_data, Some(1));
        }

        #[test]
        fn walk_roots_name_entries_after_the_closest_recursive_user_watch() {
            let mut roots =
                WalkRoots::new(WatchPath::from_parts("/repo".into(), "/repo".into(), None));
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
                None,
            ));
            let logs = user_watch("logs", true);

            roots.entry("/repo/logs".into(), Some(&logs));
            assert_eq!(
                roots.entry("/repo/logs/access".into(), None).requested,
                Path::new("logs/access")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn walk_failure_policy() {
        assert_eq!(WalkFailure::of(&ErrorKind::PathNotFound), WalkFailure::Skip);
        assert_eq!(
            WalkFailure::of(&ErrorKind::MaxFilesWatch),
            WalkFailure::Abort
        );
        assert_eq!(
            WalkFailure::of(&ErrorKind::Io(io::ErrorKind::PermissionDenied.into())),
            WalkFailure::Report
        );
        assert_eq!(
            WalkFailure::of(&ErrorKind::Generic("symbolic link loop".into())),
            WalkFailure::Report
        );
    }
}
