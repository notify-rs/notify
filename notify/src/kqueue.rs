//! Watcher implementation for the kqueue API
//!
//! The kqueue() system call provides a generic method of notifying the user
//! when an event happens or a condition holds, based on the results of small
//! pieces of kernel code termed filters.

use super::event::*;
use super::{Config, Error, EventHandler, RecursiveMode, Result, WatchMode, Watcher};
#[cfg(test)]
use crate::{BoundSender, bounded};
use crate::{ErrorKind, PathsMut, Receiver, Sender, TargetMode, unbounded};
use kqueue::{EventData, EventFilter, FilterFlag, Ident};
use rustc_hash::FxBuildHasher;
use std::collections::HashMap;
#[cfg(test)]
use std::collections::HashSet;
use std::env;
use std::fs::metadata;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use walkdir::WalkDir;

const KQUEUE: mio::Token = mio::Token(0);
const MESSAGE: mio::Token = mio::Token(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RootState {
    Missing,
    File,
    Directory,
}

/// A path the user watches. With `TargetMode::TrackPath` the path is tracked through every
/// ancestor: the ones that exist are watched, so that the root is reported removed when any of
/// them goes away, and created and watched again when it is reachable again.
#[derive(Clone, Copy, Debug)]
struct RootWatch {
    mode: WatchMode,
    state: RootState,
}

// The EventLoop will set up a mio::Poll and use it to wait for the following:
//
// -  messages telling it what to do
//
// -  events telling it that something has happened on one of the watched files.
struct EventLoop {
    running: bool,
    poll: mio::Poll,
    event_loop_waker: Arc<mio::Waker>,
    event_loop_tx: Sender<EventLoopMsg>,
    event_loop_rx: Receiver<EventLoopMsg>,
    kqueue: kqueue::Watcher,
    event_handler: Box<dyn EventHandler>,
    watches: HashMap<PathBuf, RootWatch, FxBuildHasher>,
    /// The watched paths, and whether each is only an ancestor of tracked roots.
    watch_handles: HashMap<PathBuf, bool, FxBuildHasher>,
    /// How many tracked roots lie strictly below each path.
    ancestors: HashMap<PathBuf, usize, FxBuildHasher>,
    follow_symlinks: bool,
}

/// Watcher implementation based on inotify
#[derive(Debug)]
pub struct KqueueWatcher {
    channel: Sender<EventLoopMsg>,
    waker: Arc<mio::Waker>,
}

enum EventLoopMsg {
    AddWatch(PathBuf, WatchMode, Sender<Result<()>>),
    AddWatchMultiple(Vec<(PathBuf, WatchMode)>, Sender<Result<()>>),
    RemoveWatch(PathBuf, Sender<Result<()>>),
    Shutdown,
    #[cfg(test)]
    GetWatchHandles(BoundSender<Vec<(PathBuf, bool)>>),
}

impl EventLoop {
    pub fn new(
        kqueue: kqueue::Watcher,
        event_handler: Box<dyn EventHandler>,
        follow_symlinks: bool,
    ) -> Result<Self> {
        let (event_loop_tx, event_loop_rx) = unbounded::<EventLoopMsg>();
        let poll = mio::Poll::new()?;

        let event_loop_waker = Arc::new(mio::Waker::new(poll.registry(), MESSAGE)?);

        let kqueue_fd = kqueue.as_raw_fd();
        let mut evented_kqueue = mio::unix::SourceFd(&kqueue_fd);
        poll.registry()
            .register(&mut evented_kqueue, KQUEUE, mio::Interest::READABLE)?;

        let event_loop = EventLoop {
            running: true,
            poll,
            event_loop_waker,
            event_loop_tx,
            event_loop_rx,
            kqueue,
            event_handler,
            watches: HashMap::default(),
            watch_handles: HashMap::default(),
            ancestors: HashMap::default(),
            follow_symlinks,
        };
        Ok(event_loop)
    }

    // Run the event loop.
    pub fn run(self) {
        let result = thread::Builder::new()
            .name("notify-rs kqueue loop".to_string())
            .spawn(|| self.event_loop_thread());
        if let Err(e) = result {
            tracing::error!(?e, "failed to start kqueue event loop thread");
        }
    }

    fn event_loop_thread(mut self) {
        let mut events = mio::Events::with_capacity(16);
        loop {
            // Wait for something to happen.
            match self.poll.poll(&mut events, None) {
                Err(ref e) if matches!(e.kind(), std::io::ErrorKind::Interrupted) => {
                    // System call was interrupted, we will retry
                    // TODO: Not covered by tests (to reproduce likely need to setup signal handlers)
                }
                Err(e) => panic!("poll failed: {e}"),
                Ok(()) => {}
            }

            // Process whatever happened.
            for event in &events {
                self.handle_event(event);
            }

            // Stop, if we're done.
            if !self.running {
                break;
            }
        }
    }

    // Handle a single event.
    fn handle_event(&mut self, event: &mio::event::Event) {
        match event.token() {
            MESSAGE => {
                // The channel is readable - handle messages.
                self.handle_messages();
            }
            KQUEUE => {
                // inotify has something to tell us.
                self.handle_kqueue();
            }
            _ => unreachable!(),
        }
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.event_loop_rx.try_recv() {
            match msg {
                EventLoopMsg::AddWatch(path, watch_mode, tx) => {
                    let result = tx.send(self.add_watch(path, watch_mode));
                    if let Err(e) = result {
                        tracing::error!(?e, "failed to send AddWatch result");
                    }
                }
                EventLoopMsg::AddWatchMultiple(paths, tx) => {
                    let result = tx.send(self.add_watch_multiple(paths));
                    if let Err(e) = result {
                        tracing::error!(?e, "failed to send AddWatchMultiple result");
                    }
                }
                EventLoopMsg::RemoveWatch(path, tx) => {
                    let result = tx.send(self.remove_watch(&path));
                    if let Err(e) = result {
                        tracing::error!(?e, "failed to send RemoveWatch result");
                    }
                }
                EventLoopMsg::Shutdown => {
                    self.running = false;
                    break;
                }
                #[cfg(test)]
                EventLoopMsg::GetWatchHandles(tx) => {
                    let handles = self
                        .watch_handles
                        .iter()
                        .map(|(path, chain)| (path.clone(), *chain))
                        .collect();
                    tx.send(handles).unwrap();
                }
            }
        }
    }

    fn is_watched_path(watches: &HashMap<PathBuf, RootWatch, FxBuildHasher>, path: &Path) -> bool {
        if watches.contains_key(path) {
            return true;
        }

        let Some(parent) = path.parent() else {
            return false;
        };
        if watches.contains_key(parent) {
            return true;
        }

        parent.ancestors().skip(1).any(|ancestor| {
            watches
                .get(ancestor)
                .is_some_and(|root| root.mode.recursive_mode == RecursiveMode::Recursive)
        })
    }

    /// Whether `path` is a recursive root or lies below one.
    fn is_recursive_at(watches: &HashMap<PathBuf, RootWatch, FxBuildHasher>, path: &Path) -> bool {
        watches
            .iter()
            .any(|(root, watch)| watch.mode.recursive_mode.is_recursive() && path.starts_with(root))
    }

    /// `path` is gone: a root there is missing, and the roots below it are cut off.
    fn note_gone(
        watches: &mut HashMap<PathBuf, RootWatch, FxBuildHasher>,
        ancestors: &HashMap<PathBuf, usize, FxBuildHasher>,
        path: &Path,
        vanished: &mut Vec<PathBuf>,
    ) {
        if let Some(root) = watches.get_mut(path) {
            root.state = RootState::Missing;
        }
        if ancestors.contains_key(path) {
            vanished.push(path.to_path_buf());
        }
    }

    fn note_present(
        watches: &mut HashMap<PathBuf, RootWatch, FxBuildHasher>,
        path: &Path,
        is_dir: bool,
    ) {
        if let Some(root) = watches.get_mut(path) {
            root.state = if is_dir {
                RootState::Directory
            } else {
                RootState::File
            };
        }
    }

    #[expect(clippy::too_many_lines)]
    fn handle_kqueue(&mut self) {
        let mut add_watches = Vec::new();
        let mut remove_watches = Vec::new();
        let mut vanished = Vec::new();
        let mut changed_dirs = Vec::new();

        while let Some(event) = self.kqueue.poll(None) {
            tracing::trace!(?event, "kqueue event received");

            match event {
                kqueue::Event {
                    data: EventData::Vnode(data),
                    ident: Ident::Filename(_, path),
                } => {
                    let path = PathBuf::from(path);
                    let mut evs = Vec::new();
                    match data {
                        /*
                        TODO: Differentiate folders and files
                        kqueue doesn't tell us if this was a file or a dir, so we
                        could only emulate this inotify behavior if we keep track of
                        all files and directories internally and then perform a
                        lookup.
                        */
                        kqueue::Vnode::Delete => {
                            remove_watches.push(path.clone());
                            Self::note_gone(
                                &mut self.watches,
                                &self.ancestors,
                                &path,
                                &mut vanished,
                            );
                            if Self::is_watched_path(&self.watches, &path) {
                                let remove_event = Event::new(EventKind::Remove(RemoveKind::Any))
                                    .add_path(path.clone());
                                evs.push(remove_event);
                                if let Ok(metadata) = path.metadata() {
                                    // delete event also happens when this file is overwritten by a rename
                                    // in that case, emit a create event for the new file
                                    let is_dir = metadata.is_dir();
                                    Self::note_present(&mut self.watches, &path, is_dir);
                                    add_watches.push((
                                        path.clone(),
                                        Self::is_recursive_at(&self.watches, &path),
                                        is_dir,
                                    ));
                                    tracing::trace!("overwrite detected: {}", path.display());
                                    evs.push(
                                        Event::new(EventKind::Create(if is_dir {
                                            CreateKind::Folder
                                        } else if metadata.is_file() {
                                            CreateKind::File
                                        } else {
                                            CreateKind::Other
                                        }))
                                        .add_path(path),
                                    );
                                }
                            }
                        }

                        // a write to a directory means that a new file was created in it, let's
                        // figure out which file this was
                        kqueue::Vnode::Write if path.is_dir() => {
                            if self.ancestors.contains_key(&path) {
                                changed_dirs.push(path.clone());
                            }
                            // find which file is new in the directory by comparing it with our
                            // list of known watches
                            match std::fs::read_dir(&path) {
                                Ok(dir) => {
                                    let files = dir
                                        .filter_map(std::result::Result::ok)
                                        .map(|f| f.path())
                                        .filter(|f| !self.watch_handles.contains_key(f));
                                    let mut found_new_file = false;
                                    for file in files {
                                        found_new_file = true;
                                        tracing::trace!("new file detected: {}", file.display());

                                        let metadata = file.metadata();
                                        let is_dir = metadata.as_ref().is_ok_and(|m| m.is_dir());
                                        if Self::is_watched_path(&self.watches, &file) {
                                            // watch this new file
                                            Self::note_present(&mut self.watches, &file, is_dir);
                                            add_watches.push((
                                                file.clone(),
                                                Self::is_recursive_at(&self.watches, &file),
                                                is_dir,
                                            ));

                                            evs.push(
                                                Event::new(EventKind::Create(if is_dir {
                                                    CreateKind::Folder
                                                } else if metadata.is_ok_and(|m| m.is_file()) {
                                                    CreateKind::File
                                                } else {
                                                    CreateKind::Other
                                                }))
                                                .add_path(file),
                                            );
                                            break;
                                        }
                                    }
                                    if !found_new_file
                                        && Self::is_watched_path(&self.watches, &path)
                                    {
                                        evs.push(
                                            Event::new(EventKind::Modify(ModifyKind::Data(
                                                DataChange::Any,
                                            )))
                                            .add_path(path),
                                        );
                                    }
                                }
                                Err(err) => {
                                    self.event_handler.handle_event(Err(err.into()));
                                }
                            }
                        }

                        // data was written to this file
                        kqueue::Vnode::Write => {
                            if Self::is_watched_path(&self.watches, &path) {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Data(
                                        DataChange::Any,
                                    )))
                                    .add_path(path),
                                );
                            }
                        }

                        /*
                        Extend and Truncate are just different names for the same
                        operation, extend is only used on FreeBSD, truncate everywhere
                        else
                        */
                        kqueue::Vnode::Extend | kqueue::Vnode::Truncate => {
                            if Self::is_watched_path(&self.watches, &path) {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Data(
                                        DataChange::Size,
                                    )))
                                    .add_path(path),
                                );
                            }
                        }

                        /*
                        this kevent has the same problem as the delete kevent. The
                        only way i can think of providing "better" event with more
                        information is to do the diff our self, while this maybe do
                        able of delete. In this case it would somewhat expensive to
                        keep track and compare ever peace of metadata for every file
                        */
                        kqueue::Vnode::Attrib => {
                            if Self::is_watched_path(&self.watches, &path) {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Metadata(
                                        MetadataKind::Any,
                                    )))
                                    .add_path(path),
                                );
                            }
                        }

                        /*
                        The link count on a file changed => subdirectory created or
                        delete.
                        */
                        kqueue::Vnode::Link => {
                            // As we currently don't have a solution that would allow us
                            // to only add/remove the new/delete directory and that dosn't include a
                            // possible race condition. On possible solution would be to
                            // create a `HashMap<PathBuf, Vec<PathBuf>>` which would
                            // include every directory and this content add the time of
                            // adding it to kqueue. While this should allow us to do the
                            // diff and only add/remove the files necessary. This would
                            // also introduce a race condition, where multiple files could
                            // all ready be remove from the directory, and we could get out
                            // of sync.
                            // So for now, until we find a better solution, let remove and
                            // readd the whole directory.
                            // This is a expensive operation, as we recursive through all
                            // subdirectories.
                            if Self::is_recursive_at(&self.watches, &path) {
                                remove_watches.push(path.clone());
                                add_watches.push((path.clone(), true, true));
                            }
                            if Self::is_watched_path(&self.watches, &path) {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path),
                                );
                            }
                        }

                        // Kqueue not provide us with the information necessary to provide
                        // the new file name to the event.
                        kqueue::Vnode::Rename => {
                            remove_watches.push(path.clone());
                            Self::note_gone(
                                &mut self.watches,
                                &self.ancestors,
                                &path,
                                &mut vanished,
                            );
                            if Self::is_watched_path(&self.watches, &path) {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Name(
                                        RenameMode::Any,
                                    )))
                                    .add_path(path),
                                );
                            }
                        }

                        // Access to the file was revoked via revoke(2) or the underlying file system was unmounted.
                        kqueue::Vnode::Revoke => {
                            remove_watches.push(path.clone());
                            Self::note_gone(
                                &mut self.watches,
                                &self.ancestors,
                                &path,
                                &mut vanished,
                            );
                            if Self::is_watched_path(&self.watches, &path) {
                                evs.push(
                                    Event::new(EventKind::Remove(RemoveKind::Any)).add_path(path),
                                );
                            }
                        }

                        // On different BSD variants, different extra events may be present
                        _ => evs.push(Event::new(EventKind::Other)),
                    }
                    for ev in evs {
                        self.event_handler.handle_event(Ok(ev));
                    }
                }
                // as we don't add any other EVFILTER to kqueue we should never get here
                kqueue::Event { ident: _, data: _ } => unreachable!(),
            }
        }

        tracing::trace!(
            ?add_watches,
            ?remove_watches,
            "processing kqueue watch changes"
        );

        for path in remove_watches {
            if self
                .watches
                .get(&path)
                .is_some_and(|root| root.mode.target_mode == TargetMode::NoTrack)
            {
                self.watches.remove(&path);
            }
            self.remove_maybe_recursive_watch(&path, true).ok();
        }

        for (path, is_recursive, is_dir) in add_watches {
            if let Err(err) = self.add_maybe_recursive_watch(path.clone(), is_recursive, is_dir)
                && let ErrorKind::Io(err_kind) = err.kind
                && err_kind.kind() == std::io::ErrorKind::NotFound
                && err.paths.contains(&path)
            {
                // file was deleted before we could add the watch, emit a remove event
                self.event_handler.handle_event(Ok(
                    Event::new(EventKind::Remove(RemoveKind::Any)).add_path(path)
                ));
            }
        }

        for path in vanished {
            self.vanish_below(&path);
        }
        for dir in changed_dirs {
            self.check_chain(&dir);
        }

        self.kqueue.watch().unwrap();
    }

    /// An ancestor of tracked roots changed: the roots below a child that is gone are cut off,
    /// the ones below a child that is back can be reached.
    fn check_chain(&mut self, dir: &Path) {
        let children: Vec<PathBuf> = self
            .ancestors
            .keys()
            .filter(|ancestor| ancestor.parent() == Some(dir))
            .cloned()
            .collect();
        for child in children {
            match std::fs::symlink_metadata(&child) {
                Ok(meta) if meta.is_dir() => self.rearm_below(&child),
                Ok(_) | Err(_) => self.vanish_below(&child),
            }
        }
    }

    /// The roots below `path` are out of reach: report the ones that were present, and drop the
    /// watches below, which sit on moved or deleted files.
    fn vanish_below(&mut self, path: &Path) {
        for (root, watch) in &mut self.watches {
            if watch.mode.target_mode != TargetMode::TrackPath
                || watch.state == RootState::Missing
                || root.as_path() == path
                || !root.starts_with(path)
            {
                continue;
            }
            let kind = if watch.state == RootState::Directory {
                RemoveKind::Folder
            } else {
                RemoveKind::File
            };
            watch.state = RootState::Missing;
            self.event_handler.handle_event(Ok(
                Event::new(EventKind::Remove(kind)).add_path(root.clone())
            ));
        }
        self.remove_handles_below(path);
    }

    /// `path` is a directory again: watch the roots below it that can be reached now.
    fn rearm_below(&mut self, path: &Path) {
        let roots: Vec<(PathBuf, RecursiveMode)> = self
            .watches
            .iter()
            .filter(|(root, watch)| {
                watch.mode.target_mode == TargetMode::TrackPath
                    && watch.state == RootState::Missing
                    && root.as_path() != path
                    && root.starts_with(path)
            })
            .map(|(root, watch)| (root.clone(), watch.mode.recursive_mode))
            .collect();
        for (root, recursive_mode) in roots {
            match self.arm_root(&root, recursive_mode) {
                Ok(RootState::Missing) => {}
                Ok(state) => {
                    if let Some(watch) = self.watches.get_mut(&root) {
                        watch.state = state;
                    }
                    let kind = if state == RootState::Directory {
                        CreateKind::Folder
                    } else {
                        CreateKind::File
                    };
                    self.event_handler
                        .handle_event(Ok(Event::new(EventKind::Create(kind)).add_path(root)));
                }
                Err(error) => self.event_handler.handle_event(Err(error)),
            }
        }
    }

    fn remove_handles_below(&mut self, path: &Path) {
        let handles: Vec<PathBuf> = self
            .watch_handles
            .keys()
            .filter(|handle| handle.starts_with(path))
            .cloned()
            .collect();
        for handle in handles {
            self.remove_single_watch(&handle).ok();
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn add_watch(&mut self, path: PathBuf, watch_mode: WatchMode) -> Result<()> {
        self.add_watch_inner(path, watch_mode)?;

        // Only make a single `kevent` syscall to add all the watches.
        self.kqueue.watch()?;

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn add_watch_multiple(&mut self, paths: Vec<(PathBuf, WatchMode)>) -> Result<()> {
        for (path, watch_mode) in paths {
            self.add_watch_inner(path, watch_mode)?;
        }

        // Only make a single `kevent` syscall to add all the watches.
        self.kqueue.watch()?;

        Ok(())
    }

    /// The caller of this function must call `self.kqueue.watch()` afterwards to register the new watch.
    #[tracing::instrument(level = "trace", skip(self))]
    fn add_watch_inner(&mut self, path: PathBuf, watch_mode: WatchMode) -> Result<()> {
        if let Some(existing) = self.watches.get(&path).copied() {
            let need_upgrade_to_recursive = match existing.mode.recursive_mode {
                RecursiveMode::Recursive => false,
                RecursiveMode::NonRecursive => {
                    watch_mode.recursive_mode == RecursiveMode::Recursive
                }
            };
            let need_to_track = match existing.mode.target_mode {
                TargetMode::TrackPath => false,
                TargetMode::NoTrack => watch_mode.target_mode == TargetMode::TrackPath,
            };
            tracing::trace!(
                ?need_to_track,
                ?need_upgrade_to_recursive,
                "upgrading existing watch for path: {}",
                path.display()
            );
            if need_to_track {
                self.arm_chain(&path)?;
                self.track_ancestors(&path);
            }
            if need_upgrade_to_recursive && metadata(&path).map_err(Error::io)?.is_dir() {
                self.add_maybe_recursive_watch(path.clone(), true, true)?;
            }
            self.watches
                .get_mut(&path)
                .unwrap()
                .mode
                .upgrade_with(watch_mode);
            return Ok(());
        }

        if watch_mode.target_mode == TargetMode::TrackPath {
            let state = self.arm_root(&path, watch_mode.recursive_mode)?;
            self.track_ancestors(&path);
            self.watches.insert(
                path,
                RootWatch {
                    mode: watch_mode,
                    state,
                },
            );
            return Ok(());
        }

        let meta = metadata(&path).map_err(Error::io_watch)?;
        self.add_maybe_recursive_watch(
            path.clone(),
            // If the watch is not recursive, or if we determine (by stat'ing the path to get its
            // metadata) that the watched path is not a directory, add a single path watch.
            watch_mode.recursive_mode.is_recursive() && meta.is_dir(),
            meta.is_dir(),
        )?;
        let state = if meta.is_dir() {
            RootState::Directory
        } else {
            RootState::File
        };
        self.watches.insert(
            path,
            RootWatch {
                mode: watch_mode,
                state,
            },
        );

        Ok(())
    }

    /// Watches a tracked root: its ancestors, and the root itself if it exists.
    fn arm_root(&mut self, root: &Path, recursive_mode: RecursiveMode) -> Result<RootState> {
        if !self.arm_chain(root)? {
            return Ok(RootState::Missing);
        }
        let meta = match metadata(root).map_err(Error::io_watch) {
            Ok(meta) => meta,
            Err(err) if matches!(err.kind, ErrorKind::PathNotFound) => {
                return Ok(RootState::Missing);
            }
            Err(err) => return Err(err),
        };
        self.add_maybe_recursive_watch(
            root.to_path_buf(),
            recursive_mode.is_recursive() && meta.is_dir(),
            meta.is_dir(),
        )?;
        Ok(if meta.is_dir() {
            RootState::Directory
        } else {
            RootState::File
        })
    }

    /// Watches the ancestors of `root` that exist, from the top down. Returns whether the parent
    /// exists.
    fn arm_chain(&mut self, root: &Path) -> Result<bool> {
        let Some(parent) = root.parent() else {
            return Ok(false);
        };
        let ancestors: Vec<PathBuf> = root.ancestors().skip(1).map(Path::to_path_buf).collect();
        for ancestor in ancestors.into_iter().rev() {
            if !ancestor.is_dir() {
                return Ok(false);
            }
            if ancestor == parent {
                self.add_single_watch(ancestor, false)?;
            } else if let Err(e) = self.add_single_watch(ancestor.clone(), true) {
                tracing::debug!(?e, "cannot watch ancestor: {}", ancestor.display());
            }
        }
        Ok(true)
    }

    fn track_ancestors(&mut self, root: &Path) {
        for ancestor in root.ancestors().skip(1) {
            *self.ancestors.entry(ancestor.to_path_buf()).or_insert(0) += 1;
        }
    }

    /// Forgets the ancestors of an unwatched root, dropping the watches nobody needs any more.
    fn untrack_ancestors(&mut self, root: &Path) {
        for ancestor in root.ancestors().skip(1) {
            let Some(count) = self.ancestors.get_mut(ancestor) else {
                continue;
            };
            *count -= 1;
            if *count > 0 {
                continue;
            }
            self.ancestors.remove(ancestor);
            if !self.watches.contains_key(ancestor)
                && !Self::is_recursive_at(&self.watches, ancestor)
            {
                self.remove_single_watch(ancestor).ok();
            }
        }
    }

    /// The caller of this function must call `self.kqueue.watch()` afterwards to register the new watch.
    #[tracing::instrument(level = "trace", skip(self))]
    fn add_maybe_recursive_watch(
        &mut self,
        path: PathBuf,
        is_recursive: bool,
        is_dir: bool,
    ) -> Result<()> {
        if is_recursive {
            for entry in WalkDir::new(&path).follow_links(self.follow_symlinks) {
                let entry = entry.map_err(map_walkdir_error)?;
                self.add_single_watch(entry.into_path(), false)?;
            }
        } else if is_dir {
            self.add_single_watch(path.clone(), false)?;
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.filter_map(std::result::Result::ok) {
                    self.add_single_watch(entry.path(), false)?;
                }
            }
        } else {
            self.add_single_watch(path, false)?;
        }
        Ok(())
    }

    /// Adds a single watch to the kqueue.
    ///
    /// The caller of this function must call `self.kqueue.watch()` afterwards to register the new watch.
    #[tracing::instrument(level = "trace", skip(self))]
    fn add_single_watch(&mut self, path: PathBuf, chain: bool) -> Result<()> {
        if let Some(existing_chain) = self.watch_handles.get_mut(&path) {
            tracing::trace!("watch handle already exists: {}", path.display());
            *existing_chain = *existing_chain && chain;
            return Ok(());
        }

        let event_filter = EventFilter::EVFILT_VNODE;
        let filter_flags = FilterFlag::NOTE_DELETE
            | FilterFlag::NOTE_WRITE
            | FilterFlag::NOTE_EXTEND
            | FilterFlag::NOTE_ATTRIB
            | FilterFlag::NOTE_LINK
            | FilterFlag::NOTE_RENAME
            | FilterFlag::NOTE_REVOKE;

        tracing::trace!("adding kqueue watch: {}", path.display());

        self.kqueue
            .add_filename(&path, event_filter, filter_flags)
            .map_err(|e| Error::io(e).add_path(path.clone()))?;
        self.watch_handles.insert(path, chain);

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn remove_watch(&mut self, path: &Path) -> Result<()> {
        let Some(root) = self.watches.remove(path) else {
            return Err(Error::watch_not_found());
        };
        self.remove_maybe_recursive_watch(path, root.mode.recursive_mode.is_recursive())?;
        if root.mode.target_mode == TargetMode::TrackPath {
            self.untrack_ancestors(path);
        }
        self.kqueue.watch()?;
        Ok(())
    }

    /// The caller of this function must call `self.kqueue.watch()` afterwards to register the new watch.
    #[tracing::instrument(level = "trace", skip(self))]
    fn remove_maybe_recursive_watch(&mut self, path: &Path, is_recursive: bool) -> Result<()> {
        if is_recursive {
            // By the watches we hold, not by the disk: the path may be gone already.
            self.remove_handles_below(path);
        } else {
            self.remove_single_watch(path)?;
        }
        Ok(())
    }

    /// Removes a single watch from the kqueue.
    ///
    /// The caller of this function must call `self.kqueue.watch()` afterwards to unregister the old watch.
    #[tracing::instrument(level = "trace", skip(self))]
    fn remove_single_watch(&mut self, path: &Path) -> Result<()> {
        tracing::trace!("removing kqueue watch: {}", path.display());

        self.watch_handles.remove(path);
        self.kqueue
            .remove_filename(path, EventFilter::EVFILT_VNODE)
            .map_err(|e| Error::io(e).add_path(path.to_path_buf()))?;
        Ok(())
    }
}

fn map_walkdir_error(e: walkdir::Error) -> Error {
    if e.io_error().is_some() {
        let path = e.path().map(|p| p.to_path_buf());
        // safe to unwrap otherwise we whouldn't be in this branch
        let mut err = Error::io(e.into_io_error().unwrap());
        if let Some(path) = path {
            err = err.add_path(path);
        }
        err
    } else {
        Error::generic(&e.to_string())
    }
}

struct KqueuePathsMut<'a> {
    inner: &'a mut KqueueWatcher,
    add_paths: Vec<(PathBuf, WatchMode)>,
}
impl<'a> KqueuePathsMut<'a> {
    fn new(watcher: &'a mut KqueueWatcher) -> Self {
        Self {
            inner: watcher,
            add_paths: Vec::new(),
        }
    }
}
impl PathsMut for KqueuePathsMut<'_> {
    #[tracing::instrument(level = "debug", skip(self))]
    fn add(&mut self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        self.add_paths.push((path.to_owned(), watch_mode));
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn remove(&mut self, path: &Path) -> Result<()> {
        self.inner.unwatch_inner(path)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn commit(self: Box<Self>) -> Result<()> {
        let paths = self.add_paths;
        self.inner.watch_multiple_inner(paths)
    }
}

impl KqueueWatcher {
    fn from_event_handler(
        event_handler: Box<dyn EventHandler>,
        follow_symlinks: bool,
    ) -> Result<Self> {
        let kqueue = kqueue::Watcher::new()?;
        let event_loop = EventLoop::new(kqueue, event_handler, follow_symlinks)?;
        let channel = event_loop.event_loop_tx.clone();
        let waker = Arc::clone(&event_loop.event_loop_waker);
        event_loop.run();
        Ok(KqueueWatcher { channel, waker })
    }

    fn watch_inner(&self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        let pb = if path.is_absolute() {
            path.to_owned()
        } else {
            let p = env::current_dir().map_err(Error::io)?;
            p.join(path)
        };
        let (tx, rx) = unbounded();
        let msg = EventLoopMsg::AddWatch(pb, watch_mode, tx);

        self.channel
            .send(msg)
            .map_err(|e| Error::generic(&e.to_string()))?;
        self.waker
            .wake()
            .map_err(|e| Error::generic(&e.to_string()))?;
        rx.recv().unwrap()
    }

    fn watch_multiple_inner(&self, paths: Vec<(PathBuf, WatchMode)>) -> Result<()> {
        let pbs = paths
            .into_iter()
            .map(|(path, watch_mode)| {
                if path.is_absolute() {
                    Ok((path, watch_mode))
                } else {
                    let p = env::current_dir().map_err(Error::io)?;
                    Ok((p.join(path), watch_mode))
                }
            })
            .collect::<Result<Vec<(PathBuf, WatchMode)>>>()?;
        let (tx, rx) = unbounded();
        let msg = EventLoopMsg::AddWatchMultiple(pbs, tx);

        self.channel
            .send(msg)
            .map_err(|e| Error::generic(&e.to_string()))?;
        self.waker
            .wake()
            .map_err(|e| Error::generic(&e.to_string()))?;
        rx.recv()
            .unwrap()
            .map_err(|e| Error::generic(&e.to_string()))
    }

    fn unwatch_inner(&self, path: &Path) -> Result<()> {
        let pb = if path.is_absolute() {
            path.to_owned()
        } else {
            let p = env::current_dir().map_err(Error::io)?;
            p.join(path)
        };
        let (tx, rx) = unbounded();
        let msg = EventLoopMsg::RemoveWatch(pb, tx);

        self.channel
            .send(msg)
            .map_err(|e| Error::generic(&e.to_string()))?;
        self.waker
            .wake()
            .map_err(|e| Error::generic(&e.to_string()))?;
        rx.recv()
            .unwrap()
            .map_err(|e| Error::generic(&e.to_string()))
    }
}

impl Watcher for KqueueWatcher {
    /// Create a new watcher.
    #[tracing::instrument(level = "debug", skip(event_handler))]
    fn new<F: EventHandler>(event_handler: F, config: Config) -> Result<Self> {
        Self::from_event_handler(Box::new(event_handler), config.follow_symlinks())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn watch(&mut self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        self.watch_inner(path, watch_mode)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn paths_mut<'me>(&'me mut self) -> Box<dyn PathsMut + 'me> {
        Box::new(KqueuePathsMut::new(self))
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.unwatch_inner(path)
    }

    fn kind() -> crate::WatcherKind {
        crate::WatcherKind::Kqueue
    }

    #[cfg(test)]
    /// The watches that report the watched paths, without the ancestors watched for their entries
    /// only; see [`KqueueWatcher::get_chain_handles`].
    fn get_watch_handles(&self) -> HashSet<std::path::PathBuf> {
        self.handles(|chain| !chain)
    }
}

#[cfg(test)]
impl KqueueWatcher {
    /// The ancestors of tracked paths, watched for their entries only.
    fn get_chain_handles(&self) -> HashSet<std::path::PathBuf> {
        self.handles(|chain| chain)
    }

    fn handles(&self, keep: impl Fn(bool) -> bool) -> HashSet<std::path::PathBuf> {
        let (tx, rx) = bounded(1);
        self.channel
            .send(EventLoopMsg::GetWatchHandles(tx))
            .unwrap();
        self.waker.wake().unwrap();
        rx.recv()
            .unwrap()
            .into_iter()
            .filter(|(_, chain)| keep(*chain))
            .map(|(path, _)| path)
            .collect()
    }
}

impl Drop for KqueueWatcher {
    fn drop(&mut self) {
        // we expect the event loop to live => unwrap must not panic
        self.channel.send(EventLoopMsg::Shutdown).unwrap();
        self.waker.wake().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::test::{self, *};

    fn watcher() -> (TestWatcher<KqueueWatcher>, test::Receiver) {
        channel()
    }

    #[expect(clippy::print_stdout)]
    #[test]
    fn test_remove_recursive() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let path = PathBuf::from("src");

        let mut watcher = KqueueWatcher::new(|event| println!("{event:?}"), Config::default())?;
        watcher.watch(&path, WatchMode::recursive())?;
        let result = watcher.unwatch(&path);
        assert!(
            result.is_ok(),
            "unwatch yielded error: {}",
            result.unwrap_err()
        );
        Ok(())
    }

    #[test]
    fn create_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        rx.wait_ordered_exact([
            expected(&path).modify_meta_any().optional(),
            expected(path.clone()).create_file(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), path]),
        );
    }

    #[test]
    fn create_self_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");

        watcher.watch_nonrecursively(&path);

        std::fs::File::create_new(&path).expect("create");

        rx.wait_ordered_exact([
            expected(&path).modify_meta_any().optional(),
            expected(path.clone()).create_file(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), path])
        );
    }

    #[test]
    fn create_self_file_no_track() {
        let tmpdir = testdir();
        let (mut watcher, _) = watcher();

        let path = tmpdir.path().join("entry");

        let result = watcher.watcher.watch(
            &path,
            WatchMode {
                recursive_mode: RecursiveMode::NonRecursive,
                target_mode: TargetMode::NoTrack,
            },
        );
        assert!(
            matches!(
                result,
                Err(Error {
                    paths: _,
                    kind: ErrorKind::PathNotFound
                })
            ),
            "{result:?}"
        );
    }

    #[test]
    fn create_self_file_nested() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry/nested");

        watcher.watch_nonrecursively(&path);
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));
        assert!(watcher.watcher.get_chain_handles().contains(tmpdir.path()));

        std::fs::create_dir_all(path.parent().unwrap()).expect("create");
        std::fs::File::create_new(&path).expect("create");

        rx.wait_ordered([expected(&path).create_file()]);
        assert!(
            watcher
                .get_watch_handles()
                .is_superset(&HashSet::from([tmpdir.path().join("entry")]))
        );
    }

    #[test]
    fn track_path_reports_roots_when_an_ancestor_moves_away_and_back() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let lib = tmpdir.path().join("lib");
        let a = lib.join("a.js");
        let b = lib.join("sub").join("b.js");
        let moved = tmpdir.path().join("moved");
        std::fs::create_dir_all(lib.join("sub")).expect("create_dir_all");
        std::fs::write(&a, "1").expect("write");
        std::fs::write(&b, "1").expect("write");

        watcher.watch_nonrecursively(&a);
        watcher.watch_nonrecursively(&b);
        assert!(
            watcher
                .watcher
                .get_chain_handles()
                .is_superset(&HashSet::from([
                    tmpdir.parent_path_buf(),
                    tmpdir.to_path_buf()
                ]))
        );

        std::fs::rename(&lib, &moved).expect("rename away");
        rx.wait_unordered([expected(&a).remove_file(), expected(&b).remove_file()]);
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));

        std::fs::rename(&moved, &lib).expect("rename back");
        rx.wait_unordered([expected(&a).create_file(), expected(&b).create_file()]);
        assert!(
            watcher
                .get_watch_handles()
                .is_superset(&HashSet::from([lib.clone(), lib.join("sub")]))
        );

        std::fs::write(&a, "2").expect("write");
        rx.wait_unordered([expected(&a).modify_data_any()]);
    }

    #[test]
    fn track_path_reports_a_directory_root_when_an_ancestor_is_removed() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let parent = tmpdir.path().join("parent");
        let child = parent.join("child");
        std::fs::create_dir_all(&child).expect("create_dir_all");

        watcher.watch_recursively(&child);
        std::fs::remove_dir_all(&parent).expect("remove_dir_all");
        rx.wait_unordered([expected(&child).remove_any()]);

        std::fs::create_dir_all(&child).expect("create_dir_all");
        rx.wait_unordered([expected(&child).create_folder()]);

        std::fs::File::create_new(child.join("file")).expect("create");
        rx.wait_unordered([expected(child.join("file")).create_file()]);
    }

    #[test]
    fn unwatch_drops_the_ancestor_watches() {
        let tmpdir = testdir();
        let (mut watcher, _rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::write(&path, "1").expect("write");

        watcher.watch_nonrecursively(&path);
        assert!(!watcher.watcher.get_chain_handles().is_empty());

        watcher.watcher.unwatch(&path).expect("unwatch");
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));
        assert_eq!(watcher.watcher.get_chain_handles(), HashSet::from([]));
    }

    #[test]
    fn write_file() {
        let tmpdir = testdir();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        let (mut watcher, rx) = watcher();

        watcher.watch_recursively(&tmpdir);

        std::fs::write(&path, b"123").expect("write");

        rx.wait_ordered_exact([
            expected(&path).modify_meta_any().optional(),
            expected(&path).modify_data_any(),
            expected(&path).modify_data_size(),
            expected(&path).modify_meta_any().optional(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), path]),
        );
    }

    #[test]
    fn chmod_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        let file = std::fs::File::create_new(&path).expect("create");
        let mut permissions = file.metadata().expect("metadata").permissions();
        permissions.set_readonly(true);

        watcher.watch_recursively(&tmpdir);
        file.set_permissions(permissions).expect("set_permissions");

        rx.wait_ordered_exact([expected(&path).modify_meta_any()]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), path]),
        );
    }

    #[test]
    fn rename_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        let new_path = tmpdir.path().join("renamed");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_ordered_exact([
            expected(&new_path).create_file(),
            expected(path).rename_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), new_path]),
        );
    }

    #[test]
    fn rename_self_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&path);
        let new_path = tmpdir.path().join("renamed");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_ordered_exact([expected(&path).rename_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()]),
        );

        std::fs::rename(&new_path, &path).expect("rename2");

        rx.wait_ordered_exact([expected(&path).create_file()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), path]),
        );
    }

    #[test]
    fn rename_self_file_no_track() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch(
            &path,
            WatchMode {
                recursive_mode: RecursiveMode::NonRecursive,
                target_mode: TargetMode::NoTrack,
            },
        );

        let new_path = tmpdir.path().join("renamed");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_ordered_exact([expected(&path).rename_any()])
            .ensure_no_tail();
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]),);

        let result = watcher.watcher.watch(
            &path,
            WatchMode {
                recursive_mode: RecursiveMode::NonRecursive,
                target_mode: TargetMode::NoTrack,
            },
        );
        assert!(matches!(
            result,
            Err(Error {
                paths: _,
                kind: ErrorKind::PathNotFound
            })
        ));
    }

    #[test]
    fn delete_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let file = tmpdir.path().join("file");
        std::fs::write(&file, "").expect("write");

        watcher.watch_nonrecursively(&tmpdir);

        std::fs::remove_file(&file).expect("remove");

        rx.wait_ordered_exact([
            expected(&file).modify_any(),
            expected(&file).remove_any(),
            expected(tmpdir.path()).modify_data_any(),
            expected(&file).remove_any().optional().multiple(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()]),
        );
    }

    #[test]
    fn delete_self_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let file = tmpdir.path().join("file");
        std::fs::write(&file, "").expect("write");

        watcher.watch_nonrecursively(&file);

        std::fs::remove_file(&file).expect("remove");

        rx.wait_ordered_exact([
            expected(&file).modify_any(),
            expected(&file).remove_any(),
            expected(&file).remove_any().optional().multiple(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );

        std::fs::write(&file, "").expect("write");

        rx.wait_ordered_exact([expected(&file).create_file()]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), file])
        );
    }

    #[test]
    fn delete_self_file_no_track() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let file = tmpdir.path().join("file");
        std::fs::write(&file, "").expect("write");

        watcher.watch(
            &file,
            WatchMode {
                recursive_mode: RecursiveMode::NonRecursive,
                target_mode: TargetMode::NoTrack,
            },
        );

        std::fs::remove_file(&file).expect("remove");

        rx.wait_ordered_exact([
            expected(&file).modify_any(),
            expected(&file).remove_any(),
            expected(&file).remove_any().optional().multiple(),
        ])
        .ensure_no_tail();
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));

        std::fs::write(&file, "").expect("write");

        rx.ensure_empty_with_wait();
    }

    #[test]
    fn create_write_overwrite() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let overwritten_file = tmpdir.path().join("overwritten_file");
        let overwriting_file = tmpdir.path().join("overwriting_file");
        std::fs::write(&overwritten_file, "123").expect("write1");

        watcher.watch_nonrecursively(&tmpdir);

        std::fs::File::create(&overwriting_file).expect("create");
        std::fs::write(&overwriting_file, "321").expect("write2");
        std::fs::rename(&overwriting_file, &overwritten_file).expect("rename");

        rx.wait_ordered([
            // create for overwriting_file can be
            // create_file or create_other and may be missing
            expected(&overwriting_file).modify_data_size().optional(),
            expected(&overwriting_file)
                .modify_data_any()
                .optional()
                .multiple(),
            expected(&overwritten_file).create_file(),
            expected(&overwriting_file).rename_any().optional(),
            expected(&overwriting_file).remove_any().optional(),
        ]);
        assert!(
            // overwriting_file is sometimes included
            // this happens when the rename happens right before
            // the pathname -> file descriptor resolution is done
            watcher.get_watch_handles().is_superset(&HashSet::from([
                tmpdir.parent_path_buf(),
                tmpdir.to_path_buf(),
                overwritten_file
            ]))
        );
    }

    fn assert_track_path_continues_after_recreating_file_in_nested_directory(
        upgrade_from_no_track: bool,
    ) {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();
        let nested_dir = tmpdir.path().join("nested");
        let watched_file = nested_dir.join("watched");
        let moved_file = tmpdir.path().join("moved");
        std::fs::create_dir(&nested_dir).expect("create nested dir");
        std::fs::write(&watched_file, "initial").expect("write watched file");

        watcher.watch_nonrecursively(&tmpdir);
        if upgrade_from_no_track {
            watcher.watch(
                &watched_file,
                WatchMode {
                    recursive_mode: RecursiveMode::NonRecursive,
                    target_mode: TargetMode::NoTrack,
                },
            );
        }
        watcher.watch_nonrecursively(&watched_file);

        std::fs::rename(&watched_file, &moved_file).expect("move watched file");
        std::fs::copy(&moved_file, &watched_file).expect("recreate watched file");
        std::fs::remove_file(&moved_file).expect("remove moved file");

        // Wait until the replacement events are drained before checking the next write.
        for _ in rx.iter() {}

        std::fs::write(&watched_file, "updated").expect("update watched file");
        let received_change = rx.iter().any(|event| {
            event.paths.iter().any(|path| path == &watched_file)
                && matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(ModifyKind::Data(_))
                )
        });

        assert!(
            received_change,
            "expected a change event after recreating the watched file"
        );
    }

    #[test]
    fn track_path_continues_after_recreating_file_in_nested_directory() {
        assert_track_path_continues_after_recreating_file_in_nested_directory(false);
    }

    #[test]
    fn track_path_upgrade_continues_after_recreating_file_in_nested_directory() {
        assert_track_path_continues_after_recreating_file_in_nested_directory(true);
    }

    #[test]
    fn create_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create");

        rx.wait_ordered_exact([
            expected(&path).create_folder(),
            expected(tmpdir.path()).modify_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), path]),
        );
    }

    #[test]
    fn chmod_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");
        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_readonly(true);

        watcher.watch_recursively(&tmpdir);
        std::fs::set_permissions(&path, permissions).expect("set_permissions");

        rx.wait_ordered_exact([expected(&path).modify_meta_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), path]),
        );
    }

    #[test]
    fn rename_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        let new_path = tmpdir.path().join("new_path");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_ordered_exact([
            expected(&new_path).create_folder(),
            expected(&path).rename_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), new_path]),
        );
    }

    #[test]
    fn delete_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::remove_dir(&path).expect("remove");

        rx.wait_ordered_exact([
            expected(tmpdir.path()).modify_data_any().optional(),
            expected(&path).modify_any(),
            expected(&path).remove_any(),
            expected(&path).remove_any().optional().multiple(),
            expected(tmpdir.path()).modify_data_any().optional(),
            expected(tmpdir.path()).modify_any(),
            expected(&path).remove_any().optional().multiple(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()]),
        );
    }

    #[test]
    fn delete_self_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&path);
        std::fs::remove_dir(&path).expect("remove");

        rx.wait_ordered_exact([
            expected(&path).modify_any(),
            expected(&path).remove_any(),
            expected(&path).remove_any().optional().multiple(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()]),
        );

        std::fs::create_dir(&path).expect("create_dir2");

        rx.wait_ordered_exact([expected(&path).create_folder()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), path.clone()])
        );
    }

    #[test]
    fn delete_self_dir_no_track() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");

        watcher
            .watcher
            .watch(
                &path,
                WatchMode {
                    recursive_mode: RecursiveMode::Recursive,
                    target_mode: TargetMode::NoTrack,
                },
            )
            .expect("watch");
        std::fs::remove_dir(&path).expect("remove");

        rx.wait_ordered_exact([
            expected(tmpdir.path()).modify_data_any().optional(),
            expected(&path).modify_any(),
            expected(&path).remove_any(),
            expected(tmpdir.path()).modify_data_any().optional(),
            expected(&path).remove_any().optional().multiple(),
        ])
        .ensure_no_tail();
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]),);

        std::fs::create_dir(&path).expect("create_dir2");

        rx.ensure_empty_with_wait();
    }

    #[test]
    fn rename_dir_twice() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        let new_path = tmpdir.path().join("new_path");
        let new_path2 = tmpdir.path().join("new_path2");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::rename(&path, &new_path).expect("rename");
        std::fs::rename(&new_path, &new_path2).expect("rename2");

        rx.wait_unordered([
            expected(&path).rename_any(),
            expected(&new_path2).create_folder(),
        ]);
        assert!(
            // new_path is sometimes included
            // this happens when the rename happens right before
            // the pathname -> file descriptor resolution is done
            watcher.get_watch_handles().is_superset(&HashSet::from([
                tmpdir.parent_path_buf(),
                tmpdir.to_path_buf(),
                new_path2
            ]))
        );
    }

    #[test]
    fn move_out_of_watched_dir() {
        let tmpdir = testdir();
        let subdir = tmpdir.path().join("subdir");
        let (mut watcher, rx) = watcher();

        let path = subdir.join("entry");
        std::fs::create_dir_all(&subdir).expect("create_dir_all");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&subdir);
        let new_path = tmpdir.path().join("entry");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_ordered_exact([
            expected(&subdir).modify_data_any(),
            expected(&path).rename_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), subdir])
        );
    }

    #[test]
    fn create_write_write_rename_write_remove() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let file1 = tmpdir.path().join("entry");
        let file2 = tmpdir.path().join("entry2");
        std::fs::File::create_new(&file2).expect("create file2");
        let new_path = tmpdir.path().join("renamed");

        watcher.watch_recursively(&tmpdir);
        std::fs::write(&file1, "123").expect("write 1");
        std::fs::write(&file2, "321").expect("write 2");
        std::fs::rename(&file1, &new_path).expect("rename");
        std::fs::write(&new_path, b"1").expect("write 3");
        std::fs::remove_file(&new_path).expect("remove");

        rx.wait_ordered([
            expected(&file2).modify_data_size().optional(),
            expected(&file2).modify_data_any(),
            expected(&new_path).create_file().optional(),
            expected(&file1).rename_any().optional(),
            expected(&new_path).remove_any().optional(),
        ]);
        assert!(
            // new_path is sometimes included
            // this happens when the rename happens right before
            // the pathname -> file descriptor resolution is done
            watcher.get_watch_handles().is_superset(&HashSet::from([
                tmpdir.parent_path_buf(),
                tmpdir.to_path_buf(),
                file2
            ]))
        );
    }

    #[test]
    fn rename_twice() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        let new_path1 = tmpdir.path().join("renamed1");
        let new_path2 = tmpdir.path().join("renamed2");

        std::fs::rename(&path, &new_path1).expect("rename1");
        std::fs::rename(&new_path1, &new_path2).expect("rename2");

        rx.wait_ordered([
            // create for new_path1 can be
            // create_file or create_other and may be missing
            expected(&path).rename_any().optional(),
            expected(&new_path1).remove_any().optional(),
            expected(&new_path2).create_file(),
            expected(&path).rename_any().optional(),
            expected(&new_path1).rename_any().optional(),
            expected(&new_path1).remove_any().optional(),
            expected(&new_path2).create_file().optional(),
        ])
        .ensure_no_tail();
        assert!(
            // new_path1 is sometimes included
            // this happens when the rename happens right before
            // the pathname -> file descriptor resolution is done
            watcher.get_watch_handles().is_superset(&HashSet::from([
                tmpdir.parent_path_buf(),
                tmpdir.to_path_buf(),
                new_path2
            ]))
        );
    }

    #[test]
    fn set_file_mtime() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        let file = std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);

        file.set_modified(
            std::time::SystemTime::now()
                .checked_sub(Duration::from_secs(60 * 60))
                .expect("time"),
        )
        .expect("set_time");

        rx.wait_ordered_exact([expected(&path).modify_meta_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), path]),
        );
    }

    #[test]
    fn write_file_non_recursive_watch() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_nonrecursively(&path);

        std::fs::write(&path, b"123").expect("write");

        rx.wait_ordered_exact([
            expected(&path).modify_meta_any().optional(),
            expected(&path).modify_data_any(),
            expected(&path).modify_data_size(),
            expected(&path).modify_meta_any().optional(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), path]),
        );
    }

    #[test]
    fn write_to_a_hardlink_pointed_to_the_watched_file_triggers_an_event() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let subdir = tmpdir.path().join("subdir");
        let subdir2 = tmpdir.path().join("subdir2");
        let file = subdir.join("file");
        let hardlink = tmpdir.path().join("hardlink");

        std::fs::create_dir(&subdir).expect("create");
        std::fs::create_dir(&subdir2).expect("create2");
        std::fs::write(&file, "").expect("file");
        std::fs::hard_link(&file, &hardlink).expect("hardlink");

        watcher.watch_nonrecursively(&file);

        std::fs::write(&hardlink, "123123").expect("write to the hard link");

        rx.wait_ordered_exact([
            expected(&file).modify_meta_any().optional(),
            expected(&file).modify_data_any(),
            expected(&file).modify_data_size(),
            expected(&file).modify_meta_any().optional(),
        ])
        .ensure_no_tail();
        assert_eq!(watcher.get_watch_handles(), HashSet::from([subdir, file]));
    }

    #[test]
    #[ignore = "similar to https://github.com/notify-rs/notify/issues/727"]
    fn recursive_creation() {
        let tmpdir = testdir();
        let nested1 = tmpdir.path().join("1");
        let nested2 = tmpdir.path().join("1/2");
        let nested3 = tmpdir.path().join("1/2/3");
        let nested4 = tmpdir.path().join("1/2/3/4");
        let nested5 = tmpdir.path().join("1/2/3/4/5");
        let nested6 = tmpdir.path().join("1/2/3/4/5/6");
        let nested7 = tmpdir.path().join("1/2/3/4/5/6/7");
        let nested8 = tmpdir.path().join("1/2/3/4/5/6/7/8");
        let nested9 = tmpdir.path().join("1/2/3/4/5/6/7/8/9");

        let (mut watcher, rx) = watcher();

        watcher.watch_recursively(&tmpdir);

        std::fs::create_dir_all(&nested9).expect("create_dir_all");
        rx.wait_ordered([
            expected(&nested1).create_folder(),
            expected(&nested2).create_folder(),
            expected(&nested3).create_folder(),
            expected(&nested4).create_folder(),
            expected(&nested5).create_folder(),
            expected(&nested6).create_folder(),
            expected(&nested7).create_folder(),
            expected(&nested8).create_folder(),
            expected(&nested9).create_folder(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([
                tmpdir.to_path_buf(),
                nested1,
                nested2,
                nested3,
                nested4,
                nested5,
                nested6,
                nested7,
                nested8,
                nested9
            ])
        );
    }

    #[test]
    fn upgrade_to_recursive() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("upgrade");
        let deep = tmpdir.path().join("upgrade/deep");
        let file = tmpdir.path().join("upgrade/deep/file");
        std::fs::create_dir_all(&deep).expect("create_dir");

        watcher.watch_nonrecursively(&path);
        std::fs::File::create_new(&file).expect("create");
        std::fs::remove_file(&file).expect("delete");

        rx.wait_ordered_exact([expected(&deep).modify_data_any().optional().multiple()]);

        watcher.watch_recursively(&path);
        std::fs::File::create_new(&file).expect("create");

        rx.wait_ordered_exact([
            expected(&deep).modify_data_any().optional().multiple(),
            expected(&file).create_file(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), path, deep, file])
        );
    }
}
