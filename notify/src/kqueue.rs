//! Watcher implementation for the kqueue API
//!
//! The kqueue() system call provides a generic method of notifying the user
//! when an event happens or a condition holds, based on the results of small
//! pieces of kernel code termed filters.

use super::event::*;
use super::{
    Config, Error, ErrorKind, EventHandler, EventKindMask, RecursiveMode, Result, Watcher,
};
use crate::paths::{
    WalkRoots, WatchMetadata as Watch, WatchPath, absolute_path, is_preserved_watch_root,
    preserved_watch_mode, preserved_watch_roots, recursive_user_watch_ancestor, reported_path,
};
use crate::walk::WalkFailure;
use crate::{Receiver, Sender, unbounded};
use kqueue::{EventData, EventFilter, FilterFlag, Ident};
use std::collections::{HashMap, HashSet};
use std::fs::metadata;
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use walkdir::WalkDir;

const KQUEUE: mio::Token = mio::Token(0);
const MESSAGE: mio::Token = mio::Token(1);

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
    watches: HashMap<PathBuf, Watch>,
    follow_symlinks: bool,
    event_kinds: EventKindMask,
}

/// Watcher implementation based on inotify
#[derive(Debug)]
pub struct KqueueWatcher {
    channel: Sender<EventLoopMsg>,
    waker: Arc<mio::Waker>,
}

enum EventLoopMsg {
    AddWatch(WatchPath, RecursiveMode, Sender<Result<()>>),
    RemoveWatch(PathBuf, Sender<Result<()>>),
    GetWatchedPaths(Sender<Vec<(PathBuf, RecursiveMode)>>),
    Shutdown,
}

/// Errors from one watch request or event batch. Failed paths are retried in later batches.
#[derive(Default)]
struct WatchErrors {
    reported: HashSet<PathBuf>,
    errors: Vec<Error>,
}

impl WatchErrors {
    fn handle(&mut self, error: Error, absolute: Option<&Path>, is_user_watch: bool) -> Result<()> {
        if is_user_watch {
            return Err(error);
        }
        match WalkFailure::of(&error.kind) {
            WalkFailure::Skip => {}
            WalkFailure::Report => {
                if absolute.is_none_or(|path| self.reported.insert(path.to_path_buf())) {
                    self.errors.push(error);
                }
            }
            // A previous error for this path must not hide a watch limit failure.
            WalkFailure::Abort => return Err(error),
        }
        Ok(())
    }
}

impl EventLoop {
    pub fn new(
        kqueue: kqueue::Watcher,
        event_handler: Box<dyn EventHandler>,
        follow_symlinks: bool,
        event_kinds: EventKindMask,
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
            watches: HashMap::new(),
            follow_symlinks,
            event_kinds,
        };
        Ok(event_loop)
    }

    // Run the event loop.
    pub fn run(self) {
        let _ = thread::Builder::new()
            .name("notify-rs kqueue loop".to_string())
            .spawn(|| self.event_loop_thread());
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
                self.handle_messages()
            }
            KQUEUE => {
                // inotify has something to tell us.
                self.handle_kqueue()
            }
            _ => unreachable!(),
        }
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.event_loop_rx.try_recv() {
            match msg {
                EventLoopMsg::AddWatch(path, recursive_mode, tx) => {
                    let _ = tx.send(self.add_watch(path, recursive_mode.is_recursive(), true));
                }
                EventLoopMsg::RemoveWatch(path, tx) => {
                    let _ = tx.send(self.remove_watch(path, false));
                }
                EventLoopMsg::GetWatchedPaths(tx) => {
                    let _ = tx.send(
                        self.watches
                            .iter()
                            .filter(|(_path, watch)| watch.is_user_watch)
                            .map(|(_path, watch)| {
                                (
                                    watch.reported_path.clone(),
                                    if watch.user_is_recursive {
                                        RecursiveMode::Recursive
                                    } else {
                                        RecursiveMode::NonRecursive
                                    },
                                )
                            })
                            .collect(),
                    );
                }
                EventLoopMsg::Shutdown => {
                    self.running = false;
                    break;
                }
            }
        }
    }

    fn handle_kqueue(&mut self) {
        let mut remove_watches = Vec::new();
        let mut refresh_dirs = Vec::new();
        let mut pending_events = Vec::new();
        let mut errors = WatchErrors::default();
        let mut abort_additions = false;

        while let Some(event) = self.kqueue.poll(None) {
            log::trace!("kqueue event: {event:?}");

            match event {
                kqueue::Event {
                    data: EventData::Vnode(data),
                    ident: Ident::Filename(_, path),
                } => {
                    let path = PathBuf::from(path);
                    let watch = self.watches.get(&path);
                    let event_path = watch
                        .map(|watch| watch.reported_path.clone())
                        .unwrap_or_else(|| path.clone());
                    let event = match data {
                        // kqueue does not report the deleted entry's type, and watches
                        // do not retain it, so removals use RemoveKind::Any.
                        kqueue::Vnode::Delete => {
                            remove_watches.push((path.clone(), true));
                            Ok(Event::new(EventKind::Remove(RemoveKind::Any)).add_path(event_path))
                        }

                        // A write to a recursively watched directory may mean that a new file
                        // was created in it. Non-recursive directory watches do not track
                        // children, so guessing from read_dir would be unreliable.
                        // Non-recursive watches report the directory's data change instead.
                        // Context: https://github.com/notify-rs/notify/issues/644
                        kqueue::Vnode::Write
                            if watch.is_some_and(|watch| watch.is_recursive)
                                && if self.follow_symlinks {
                                    path.is_dir()
                                } else {
                                    std::fs::symlink_metadata(&path)
                                        .is_ok_and(|metadata| metadata.is_dir())
                                } =>
                        {
                            if !abort_additions {
                                refresh_dirs
                                    .push(WatchPath::from_parts(path.clone(), event_path.clone()));
                            }
                            let error_path = event_path.clone();
                            // find which file is new in the directory by comparing it with our
                            // list of known watches
                            std::fs::read_dir(&path)
                                .map(|dir| {
                                    dir.filter_map(std::result::Result::ok)
                                        .map(|f| f.path())
                                        .find(|f| !self.watches.contains_key(f))
                                })
                                .map(|file| {
                                    if let Some(file) = file {
                                        let reported_file =
                                            reported_path(&path, &event_path, &file);
                                        Event::new(EventKind::Create(if file.is_dir() {
                                            CreateKind::Folder
                                        } else if file.is_file() {
                                            CreateKind::File
                                        } else {
                                            CreateKind::Other
                                        }))
                                        .add_path(reported_file)
                                    } else {
                                        Event::new(EventKind::Modify(ModifyKind::Data(
                                            DataChange::Any,
                                        )))
                                        .add_path(event_path)
                                    }
                                })
                                .map_err(|err| map_kqueue_io_error(err).add_path(error_path))
                        }

                        // data was written to this file
                        kqueue::Vnode::Write => Ok(Event::new(EventKind::Modify(
                            ModifyKind::Data(DataChange::Any),
                        ))
                        .add_path(event_path)),

                        /*
                        Extend and Truncate are just different names for the same
                        operation, extend is only used on FreeBSD, truncate everywhere
                        else
                        */
                        kqueue::Vnode::Extend | kqueue::Vnode::Truncate => Ok(Event::new(
                            EventKind::Modify(ModifyKind::Data(DataChange::Size)),
                        )
                        .add_path(event_path)),

                        /*
                        this kevent has the same problem as the delete kevent. The
                        only way i can think of providing "better" event with more
                        information is to do the diff our self, while this maybe do
                        able of delete. In this case it would somewhat expensive to
                        keep track and compare ever peace of metadata for every file
                        */
                        kqueue::Vnode::Attrib => Ok(Event::new(EventKind::Modify(
                            ModifyKind::Metadata(MetadataKind::Any),
                        ))
                        .add_path(event_path)),

                        // The link count changed, for example because a subdirectory was created
                        // or removed. mkdir, rmdir and rename also post NOTE_WRITE on the
                        // directory. The NOTE_WRITE handler adds watches for the new entries.
                        kqueue::Vnode::Link => {
                            Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(event_path))
                        }

                        // Kqueue not provide us with the information necessary to provide
                        // the new file name to the event.
                        kqueue::Vnode::Rename => {
                            remove_watches.push((path.clone(), true));
                            Ok(
                                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
                                    .add_path(event_path),
                            )
                        }

                        // Access to the file was revoked via revoke(2) or the underlying file system was unmounted.
                        kqueue::Vnode::Revoke => {
                            remove_watches.push((path.clone(), true));
                            Ok(Event::new(EventKind::Remove(RemoveKind::Any)).add_path(event_path))
                        }

                        // On different BSD variants, different extra events may be present
                        #[allow(unreachable_patterns)]
                        _ => Ok(Event::new(EventKind::Other)),
                    };
                    // Filter events based on EventKindMask
                    // Errors always pass through, OK events only if they match the mask
                    match event {
                        Ok(e) if !self.event_kinds.matches(&e.kind) => {
                            // Event filtered out
                        }
                        Ok(e) => pending_events.push(Ok(e)),
                        Err(err) => {
                            if let Err(err) = errors.handle(err, Some(&path), false) {
                                if !abort_additions {
                                    errors.errors.push(err);
                                }
                                abort_additions = true;
                            }
                        }
                    }
                }
                // as we don't add any other EVFILTER to kqueue we should never get here
                kqueue::Event { ident: _, data: _ } => unreachable!(),
            }
        }

        for (path, remove_recursive) in remove_watches {
            self.remove_watch(path, remove_recursive).ok();
        }

        // Until the removals above run, an entry that was removed and created again under the
        // same name still has the watch of the removed entry. A scan before the removals would
        // skip the new entry.
        let mut add_watches = Vec::new();
        let mut scanned = HashSet::new();
        for dir in refresh_dirs {
            if abort_additions {
                break;
            }
            // If the directory's watch was removed above, for example because the directory was
            // moved away, the path now names a different directory or nothing.
            if !self.watches.contains_key(&dir.absolute) || !scanned.insert(dir.absolute.clone()) {
                continue;
            }
            match self.untracked_entries(&dir, &mut errors) {
                Ok(entries) => add_watches.extend(entries),
                Err(err) => {
                    errors.errors.push(err);
                    abort_additions = true;
                }
            }
        }

        let watch_count = self.watches.len();
        for path in add_watches {
            if abort_additions {
                break;
            }
            // Overlapping refreshes can discover an entry already added by another walk.
            if self.watches.contains_key(&path.absolute) {
                continue;
            }
            if let Err(err) = self.add_watch_entries(path, true, false, &mut errors) {
                errors.errors.push(err);
                abort_additions = true;
            }
        }

        // watch() registers all watches, so call it once for the batch, including any watches
        // added before a walk failed.
        if self.watches.len() > watch_count {
            if let Err(err) = self.kqueue.watch() {
                errors.errors.push(map_kqueue_io_error(err));
            }
        }

        // Apply recursive watch changes before reporting the events that caused them. Event
        // handlers may react to a newly created directory immediately; once its create event is
        // delivered, changes made by that handler must be covered by the new directory watch.
        for event in errors.errors.into_iter().map(Err).chain(pending_events) {
            self.event_handler.handle_event(event);
        }
    }

    fn untracked_entries(
        &self,
        dir: &WatchPath,
        errors: &mut WatchErrors,
    ) -> Result<Vec<WatchPath>> {
        let entries = match std::fs::read_dir(&dir.absolute) {
            Ok(entries) => entries,
            Err(err) => {
                errors.handle(
                    map_kqueue_io_error(err).add_path(dir.requested.clone()),
                    Some(&dir.absolute),
                    false,
                )?;
                return Ok(Vec::new());
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if !self.watches.contains_key(&path) {
                        paths.push(dir.child(path));
                    }
                }
                Err(err) => {
                    // The parent is known, but not which entry failed. Do not deduplicate these.
                    errors.handle(
                        map_kqueue_io_error(err).add_path(dir.requested.clone()),
                        None,
                        false,
                    )?;
                }
            }
        }
        Ok(paths)
    }

    fn add_watch(
        &mut self,
        path: WatchPath,
        is_recursive: bool,
        is_user_watch: bool,
    ) -> Result<()> {
        let mut errors = WatchErrors::default();
        let result = self.add_watch_entries(path, is_recursive, is_user_watch, &mut errors);
        // Register the watches that were added even if adding failed partway. A watch that is in
        // `self.watches` but not registered never reports the deletion of its path, so it would
        // never be removed.
        let registration = self.kqueue.watch().map_err(map_kqueue_io_error);
        for error in errors.errors {
            self.event_handler.handle_event(Err(error));
        }
        registration.and(result)
    }

    fn add_watch_entries(
        &mut self,
        path: WatchPath,
        is_recursive: bool,
        is_user_watch: bool,
        errors: &mut WatchErrors,
    ) -> Result<()> {
        let path_is_dir = match metadata(&path.absolute) {
            Ok(metadata) => metadata.is_dir(),
            Err(err) => {
                return errors.handle(
                    map_kqueue_io_error(err).add_path(path.requested),
                    Some(&path.absolute),
                    is_user_watch,
                );
            }
        };
        let requested_is_recursive = is_recursive && path_is_dir;
        if is_user_watch {
            if let Some(watch) = self
                .watches
                .get(&path.absolute)
                .filter(|watch| watch.is_user_watch)
            {
                if watch.user_is_recursive == requested_is_recursive
                    && watch.reported_path == path.requested
                {
                    return Ok(());
                }

                // Rewatching an explicit user watch replaces its requested mode and reported path
                // instead of merging with the previous metadata. If the current entry also carries
                // recursive coverage from an ancestor, remember that ancestor before removal so we
                // can rebuild that inherited coverage below.
                let inherited_recursive_root =
                    if !requested_is_recursive && path_is_dir && watch.is_recursive {
                        recursive_user_watch_ancestor(&path.absolute, self.watches.iter())
                    } else {
                        None
                    };
                let replaced_path = path.absolute.clone();
                self.remove_watch(replaced_path.clone(), false)?;

                if let Some((ancestor_path, ancestor_reported_path)) = inherited_recursive_root {
                    // Removing a directory watch removes its recursively inherited children too.
                    // Re-add them as non-user watches so the ancestor recursive watch still covers
                    // this subtree after the user watch is replaced.
                    let requested =
                        reported_path(&ancestor_path, &ancestor_reported_path, &replaced_path);
                    self.add_recursive_watch(
                        WatchPath::from_parts(replaced_path, requested),
                        false,
                        errors,
                    )?;
                }
            }
        }

        // If the watch is not recursive, or if we determine (by stat'ing the path to get its
        // metadata) that the watched path is not a directory, add a single path watch.
        if !requested_is_recursive {
            let absolute = path.absolute.clone();
            if let Err(err) = self.add_single_watch(path, false, is_user_watch) {
                errors.handle(err, Some(&absolute), is_user_watch)?;
            }
        } else {
            self.add_recursive_watch(path, is_user_watch, errors)?;
        }

        Ok(())
    }

    fn add_recursive_watch(
        &mut self,
        root: WatchPath,
        is_user_watch: bool,
        errors: &mut WatchErrors,
    ) -> Result<()> {
        let mut walk = WalkDir::new(&root.absolute)
            .follow_links(self.follow_symlinks)
            .into_iter();
        let mut roots = WalkRoots::new(root);
        while let Some(entry) = walk.next() {
            if self.add_walk_entry(entry, &mut roots, is_user_watch, errors)? {
                walk.skip_current_dir();
            }
        }
        Ok(())
    }

    /// Add one walk entry, returning whether to skip its directory after a failed registration.
    fn add_walk_entry(
        &mut self,
        entry: walkdir::Result<walkdir::DirEntry>,
        roots: &mut WalkRoots,
        is_user_watch: bool,
        errors: &mut WatchErrors,
    ) -> Result<bool> {
        let (depth, absolute, result, is_dir) = match entry {
            Ok(entry) => {
                let depth = entry.depth();
                let is_dir = entry.file_type().is_dir();
                let absolute = entry.into_path();
                let path = roots.entry(absolute.clone(), self.watches.get(&absolute));
                let result = self.add_single_watch(path, true, is_user_watch && depth == 0);
                (depth, Some(absolute), result, is_dir)
            }
            Err(err) => {
                let depth = err.depth();
                let absolute = err.path().map(Path::to_path_buf);
                let requested = absolute
                    .as_ref()
                    .map(|path| roots.entry(path.clone(), self.watches.get(path)).requested);
                let error = map_walkdir_error(err).set_paths(requested.into_iter().collect());
                (depth, absolute, Err(error), false)
            }
        };
        let skip_directory = is_dir && result.is_err();
        if let Err(err) = result {
            // WalkDir can yield the root and then a depth-zero error from reading it.
            errors.handle(err, absolute.as_deref(), is_user_watch && depth == 0)?;
        }
        Ok(skip_directory)
    }

    /// Adds a single watch to the kqueue.
    ///
    /// The caller of this function must call `self.kqueue.watch()` afterwards to register the new watch.
    fn add_single_watch(
        &mut self,
        path: WatchPath,
        is_recursive: bool,
        is_user_watch: bool,
    ) -> Result<()> {
        // add_filename opens the path for reading, which can block on a FIFO. Check the
        // target type, including through symlinks, before either watch-addition path opens it.
        let file_type = metadata(&path.absolute)
            .map_err(|err| map_kqueue_io_error(err).add_path(path.requested.clone()))?
            .file_type();
        if !file_type.is_file() && !file_type.is_dir() {
            if is_user_watch {
                return Err(Error::io(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "kqueue only supports watching regular files and directories",
                ))
                .add_path(path.requested));
            }
            log::debug!(
                "skipping kqueue watch for special file: {}",
                path.absolute.display()
            );
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

        log::trace!("adding kqueue watch: {}", path.absolute.display());

        self.kqueue
            .add_filename(&path.absolute, event_filter, filter_flags)
            .map_err(|e| map_kqueue_io_error(e).add_path(path.requested.clone()))?;
        let existing_watch = self.watches.get(&path.absolute);
        let watch = Watch::new(&path, is_recursive, is_user_watch, existing_watch);
        self.watches.insert(path.absolute, watch);

        Ok(())
    }

    fn remove_watch(&mut self, path: PathBuf, remove_recursive: bool) -> Result<()> {
        log::trace!("removing kqueue watch: {}", path.display());

        let preserved_roots = preserved_watch_roots(&path, remove_recursive, self.watches.iter());

        match self.watches.remove(&path) {
            None => return Err(Error::watch_not_found()),
            Some(watch) => {
                if watch.is_recursive || remove_recursive {
                    self.kqueue
                        .remove_filename(&path, EventFilter::EVFILT_VNODE)
                        .map_err(|e| Error::io(e).add_path(path.clone()))?;

                    let mut remove_list = Vec::new();
                    let mut reset_list = Vec::new();
                    for p in self.watches.keys().filter(|p| p.starts_with(&path)) {
                        if let Some(user_is_recursive) = preserved_watch_mode(p, &preserved_roots) {
                            if !user_is_recursive || is_preserved_watch_root(p, &preserved_roots) {
                                reset_list.push(p.clone());
                            }
                            continue;
                        }

                        remove_list.push(p.clone());
                    }

                    for p in &remove_list {
                        self.kqueue
                            .remove_filename(p, EventFilter::EVFILT_VNODE)
                            .map_err(|e| Error::io(e).add_path(p.clone()))?;
                    }
                    for p in remove_list {
                        self.watches.remove(&p);
                    }
                    for p in reset_list {
                        if let Some(watch) = self.watches.get_mut(&p) {
                            watch.is_recursive = watch.user_is_recursive;
                        }
                    }
                } else {
                    self.kqueue
                        .remove_filename(&path, EventFilter::EVFILT_VNODE)
                        .map_err(|e| Error::io(e).add_path(path.clone()))?;
                }

                self.kqueue.watch()?;
            }
        }
        Ok(())
    }
}

fn map_walkdir_error(e: walkdir::Error) -> Error {
    if e.io_error().is_some() {
        // Safe to unwrap because the error has an I/O source.
        map_kqueue_io_error(e.into_io_error().unwrap())
    } else {
        Error::generic(&e.to_string())
    }
}

fn map_kqueue_io_error(error: io::Error) -> Error {
    match error.raw_os_error() {
        Some(libc::EMFILE) | Some(libc::ENFILE) => Error::new(ErrorKind::MaxFilesWatch),
        _ => Error::io_watch(error),
    }
}

impl KqueueWatcher {
    fn from_event_handler(
        event_handler: Box<dyn EventHandler>,
        follow_symlinks: bool,
        event_kinds: EventKindMask,
    ) -> Result<Self> {
        let kqueue = kqueue::Watcher::new()?;
        let event_loop = EventLoop::new(kqueue, event_handler, follow_symlinks, event_kinds)?;
        let channel = event_loop.event_loop_tx.clone();
        let waker = event_loop.event_loop_waker.clone();
        event_loop.run();
        Ok(KqueueWatcher { channel, waker })
    }

    fn watch_inner(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        let pb = WatchPath::new(path)?;
        let (tx, rx) = unbounded();
        let msg = EventLoopMsg::AddWatch(pb, recursive_mode, tx);

        self.channel
            .send(msg)
            .map_err(|e| Error::generic(&e.to_string()))?;
        self.waker
            .wake()
            .map_err(|e| Error::generic(&e.to_string()))?;
        rx.recv().map_err(|e| Error::generic(&e.to_string()))?
    }

    fn unwatch_inner(&mut self, path: &Path) -> Result<()> {
        let pb = absolute_path(path)?;
        let (tx, rx) = unbounded();
        let msg = EventLoopMsg::RemoveWatch(pb, tx);

        self.channel
            .send(msg)
            .map_err(|e| Error::generic(&e.to_string()))?;
        self.waker
            .wake()
            .map_err(|e| Error::generic(&e.to_string()))?;
        rx.recv().map_err(|e| Error::generic(&e.to_string()))?
    }

    fn watched_paths_inner(&self) -> Result<Vec<(PathBuf, RecursiveMode)>> {
        let (tx, rx) = unbounded();
        self.channel.send(EventLoopMsg::GetWatchedPaths(tx))?;
        self.waker.wake()?;
        rx.recv().map_err(Error::from)
    }
}

impl Watcher for KqueueWatcher {
    /// Create a new watcher.
    fn new<F: EventHandler>(event_handler: F, config: Config) -> Result<Self> {
        Self::from_event_handler(
            Box::new(event_handler),
            config.follow_symlinks(),
            config.event_kinds(),
        )
    }

    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        self.watch_inner(path, recursive_mode)
    }

    fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.unwatch_inner(path)
    }

    fn watched_paths(&self) -> Result<Vec<(PathBuf, RecursiveMode)>> {
        self.watched_paths_inner()
    }

    fn kind() -> crate::WatcherKind {
        crate::WatcherKind::Kqueue
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

    fn test_event_loop() -> std::result::Result<EventLoop, Box<dyn std::error::Error>> {
        let kqueue = kqueue::Watcher::new()?;
        Ok(EventLoop::new(
            kqueue,
            Box::new(|_| {}),
            false,
            EventKindMask::ALL,
        )?)
    }

    fn recording_event_loop() -> (EventLoop, std::sync::mpsc::Receiver<Result<Event>>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let event_loop = EventLoop::new(
            kqueue::Watcher::new().unwrap(),
            Box::new(tx),
            true,
            EventKindMask::ALL,
        )
        .unwrap();
        (event_loop, rx)
    }

    fn assert_watch_does_not_block(test: impl FnOnce() + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            test();
            let _ = tx.send(());
        });
        // A regression must fail instead of hanging the test suite on a FIFO without a writer.
        if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
            rx.recv_timeout(Duration::from_secs(10))
        {
            panic!("watching special files did not finish");
        }
        worker.join().unwrap();
    }

    #[test]
    fn watch_io_error_kinds() {
        for errno in [libc::EMFILE, libc::ENFILE] {
            let error = map_kqueue_io_error(io::Error::from_raw_os_error(errno));
            assert!(matches!(error.kind, ErrorKind::MaxFilesWatch));
            assert_eq!(WalkFailure::of(&error.kind), WalkFailure::Abort);
        }
        let error = map_kqueue_io_error(io::Error::from_raw_os_error(libc::ENOENT));
        assert!(matches!(error.kind, ErrorKind::PathNotFound));
        let error = map_kqueue_io_error(io::Error::from_raw_os_error(libc::EACCES));
        assert!(
            matches!(error.kind, ErrorKind::Io(err) if err.raw_os_error() == Some(libc::EACCES))
        );
    }

    #[test]
    fn reported_error_does_not_hide_root_or_limit_failures() {
        let path = Path::new("/unreadable");
        let mut errors = WatchErrors::default();
        let denied = || Error::io(io::ErrorKind::PermissionDenied.into()).add_path(path.into());
        errors.handle(denied(), Some(path), false).unwrap();
        errors.handle(denied(), Some(path), false).unwrap();
        assert_eq!(errors.errors.len(), 1);

        assert!(errors.handle(denied(), Some(path), true).is_err());
        assert!(
            errors
                .handle(Error::path_not_found(), Some(path), true)
                .is_err()
        );
        let error = errors
            .handle(Error::new(ErrorKind::MaxFilesWatch), Some(path), false)
            .unwrap_err();
        assert!(matches!(error.kind, ErrorKind::MaxFilesWatch));

        // Distinct unknown entries can fail while reading the same directory.
        errors.handle(denied(), None, false).unwrap();
        errors.handle(denied(), None, false).unwrap();
        assert_eq!(errors.errors.len(), 3);
    }

    #[test]
    fn missing_root_returns_path_not_found() {
        let dir = testdir();
        let (mut event_loop, rx) = recording_event_loop();
        let root = WatchPath::from_parts(dir.path().join("missing"), "reported/missing".into());
        let error = event_loop.add_watch(root, true, true).unwrap_err();
        assert!(matches!(error.kind, ErrorKind::PathNotFound));
        assert_eq!(error.paths, [PathBuf::from("reported/missing")]);
        assert!(event_loop.watches.is_empty());
        assert!(rx.try_iter().next().is_none());
    }

    #[test]
    fn unreadable_root_returns_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = testdir();
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let (mut event_loop, rx) = recording_event_loop();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::read_dir(&root).is_ok();
        let result = event_loop.add_watch(WatchPath::new(&root).unwrap(), true, true);
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            return;
        }
        let error = result.unwrap_err();
        assert!(
            matches!(error.kind, ErrorKind::Io(err) if err.kind() == io::ErrorKind::PermissionDenied)
        );
        assert_eq!(error.paths, [root]);
        assert!(rx.try_iter().next().is_none());
    }

    #[test]
    fn root_read_error_after_root_entry_is_still_returned() {
        use std::os::unix::fs::PermissionsExt;

        let dir = testdir();
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let (mut event_loop, rx) = recording_event_loop();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::read_dir(&root).is_ok();
        let mut walk = WalkDir::new(&root).into_iter();
        let entry = walk.next();
        let read_error = walk.next();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            return;
        }

        // Keep WalkDir's actual root/read-error sequence, but let opening the root succeed.
        // This reproduces a deferred read failure without racing a permission-changing thread.
        let mut roots = WalkRoots::new(WatchPath::from_parts(root.clone(), "reported".into()));
        let mut errors = WatchErrors::default();
        assert!(
            !event_loop
                .add_walk_entry(entry.unwrap(), &mut roots, true, &mut errors)
                .unwrap()
        );
        assert!(event_loop.watches.contains_key(&root));
        let error = event_loop
            .add_walk_entry(read_error.unwrap(), &mut roots, true, &mut errors)
            .unwrap_err();
        assert!(
            matches!(error.kind, ErrorKind::Io(err) if err.kind() == io::ErrorKind::PermissionDenied)
        );
        assert_eq!(error.paths, [PathBuf::from("reported")]);
        assert!(errors.errors.is_empty());
        assert!(rx.try_iter().next().is_none());
    }

    #[test]
    fn walk_reports_cycles_and_skips_missing_children() {
        let dir = testdir();
        let root = dir.path();
        let child = root.join("child");
        let file = child.join("file");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(&file, "").unwrap();
        std::os::unix::fs::symlink("missing", root.join("dangling")).unwrap();
        std::os::unix::fs::symlink(".", root.join("cycle")).unwrap();
        let (mut event_loop, rx) = recording_event_loop();

        event_loop
            .add_watch(
                WatchPath::from_parts(root.into(), "reported".into()),
                true,
                true,
            )
            .unwrap();
        let errors: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0].kind, ErrorKind::Generic(_)));
        assert_eq!(errors[0].paths, [PathBuf::from("reported/cycle")]);
        assert!(event_loop.watches.contains_key(&file));

        std::fs::write(&file, "changed").unwrap();
        event_loop.handle_kqueue();
        assert!(rx.try_iter().any(|event| event.is_ok_and(|event| {
            event.kind.is_modify() && event.paths == [PathBuf::from("reported/child/file")]
        })));
    }

    #[test]
    fn recursive_watches_skip_special_files() {
        assert_watch_does_not_block(|| {
            for follow_symlinks in [false, true] {
                for triggered_by_event in [false, true] {
                    let dir = testdir();
                    let root = dir.path();
                    let (mut event_loop, rx) = recording_event_loop();
                    event_loop.follow_symlinks = follow_symlinks;
                    if triggered_by_event {
                        event_loop
                            .add_watch(WatchPath::new(root).unwrap(), true, true)
                            .unwrap();
                    }

                    let child = root.join("child");
                    let file = child.join("file");
                    std::fs::create_dir(&child).unwrap();
                    std::fs::write(&file, "").unwrap();
                    for fifo in [root.join("fifo"), child.join("fifo")] {
                        nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
                    }
                    let _socket =
                        std::os::unix::net::UnixListener::bind(root.join("socket")).unwrap();
                    std::os::unix::fs::symlink("fifo", root.join("fifo-link")).unwrap();
                    std::os::unix::fs::symlink("socket", root.join("socket-link")).unwrap();
                    std::os::unix::fs::symlink("/dev/null", root.join("device-link")).unwrap();
                    std::os::unix::fs::symlink("child/file", root.join("file-link")).unwrap();
                    std::os::unix::fs::symlink("child", root.join("dir-link")).unwrap();

                    if triggered_by_event {
                        event_loop.handle_kqueue();
                    } else {
                        event_loop
                            .add_watch(WatchPath::new(root).unwrap(), true, true)
                            .unwrap();
                    }
                    for path in [
                        "fifo",
                        "child/fifo",
                        "fifo-link",
                        "socket",
                        "socket-link",
                        "device-link",
                    ] {
                        assert!(!event_loop.watches.contains_key(&root.join(path)), "{path}");
                    }
                    for path in ["child", "child/file", "file-link", "dir-link"] {
                        assert!(event_loop.watches.contains_key(&root.join(path)), "{path}");
                    }
                    if follow_symlinks {
                        assert!(event_loop.watches.contains_key(&root.join("dir-link/file")));
                        assert!(!event_loop.watches.contains_key(&root.join("dir-link/fifo")));
                    }
                    assert!(rx.try_iter().all(|event| event.is_ok()));

                    // Ordinary files remain covered even when special files were skipped.
                    std::fs::write(&file, "changed").unwrap();
                    event_loop.handle_kqueue();
                    let events: Vec<_> = rx.try_iter().map(Result::unwrap).collect();
                    assert!(
                        events
                            .iter()
                            .any(|event| { event.kind.is_modify() && event.paths.contains(&file) })
                    );

                    // Skipping a FIFO must not exclude a regular file that later replaces it.
                    let replaced = root.join("fifo");
                    std::fs::remove_file(&replaced).unwrap();
                    std::fs::write(&replaced, "regular file").unwrap();
                    event_loop.handle_kqueue();
                    assert!(event_loop.watches.contains_key(&replaced));
                    assert!(rx.try_iter().all(|event| event.is_ok()));
                    std::fs::write(&replaced, "changed").unwrap();
                    event_loop.handle_kqueue();
                    let events: Vec<_> = rx.try_iter().map(Result::unwrap).collect();
                    assert!(events.iter().any(|event| {
                        event.kind.is_modify() && event.paths.contains(&replaced)
                    }));
                }
            }
        });
    }

    #[test]
    fn special_file_roots_return_errors() {
        assert_watch_does_not_block(|| {
            let dir = testdir();
            let fifo = dir.path().join("fifo");
            nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
            let socket = dir.path().join("socket");
            let _socket = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            let link = dir.path().join("link");
            std::os::unix::fs::symlink("fifo", &link).unwrap();
            let (mut event_loop, rx) = recording_event_loop();

            for is_recursive in [false, true] {
                for path in [&fifo, &socket, &link, &PathBuf::from("/dev/null")] {
                    let error = event_loop
                        .add_watch(
                            WatchPath::from_parts(path.clone(), "reported".into()),
                            is_recursive,
                            true,
                        )
                        .unwrap_err();
                    assert!(
                        matches!(error.kind, ErrorKind::Io(err) if err.kind() == io::ErrorKind::Unsupported)
                    );
                    assert_eq!(error.paths, [PathBuf::from("reported")]);
                }
            }
            assert!(event_loop.watches.is_empty());
            assert!(rx.try_iter().next().is_none());
        });
    }

    #[test]
    fn failed_entries_are_retried_after_parent_writes() {
        use std::os::unix::fs::PermissionsExt;

        let dir = testdir();
        let root = dir.path();
        let locked = root.join("locked");
        std::fs::write(&locked, "").unwrap();
        let (mut event_loop, rx) = recording_event_loop();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::File::open(&locked).is_ok();

        let result = event_loop.add_watch(WatchPath::new(root).unwrap(), true, true);
        let initial: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
        let created = std::fs::write(root.join("first"), "");
        event_loop.handle_kqueue();
        let retried: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
        if readable {
            return;
        }
        result.unwrap();
        created.unwrap();
        for errors in [initial, retried] {
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].paths, [locked.clone()]);
        }

        // A later parent write retries the now-readable file; no persistent exclusion hides it.
        std::fs::write(root.join("second"), "").unwrap();
        event_loop.handle_kqueue();
        assert!(event_loop.watches.contains_key(&locked));
        assert!(rx.try_iter().all(|event| event.is_ok()));
        std::fs::write(&locked, "changed").unwrap();
        event_loop.handle_kqueue();
        assert!(rx.try_iter().any(|event| {
            event.is_ok_and(|event| event.kind.is_modify() && event.paths.contains(&locked))
        }));
    }

    #[test]
    fn refresh_reports_unreadable_directory_once_per_batch() {
        use std::os::unix::fs::PermissionsExt;

        let dir = testdir();
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let (mut event_loop, rx) = recording_event_loop();
        event_loop.event_kinds = EventKindMask::empty();
        event_loop
            .add_watch(WatchPath::new(&root).unwrap(), true, true)
            .unwrap();
        std::fs::write(root.join("file"), "").unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::read_dir(&root).is_ok();
        event_loop.handle_kqueue();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            return;
        }
        let errors: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].paths, [root]);
        assert!(
            matches!(&errors[0].kind, ErrorKind::Io(err) if err.kind() == io::ErrorKind::PermissionDenied)
        );
    }

    #[test]
    #[ignore = "changes the process FD limit; run alone with --ignored --exact --test-threads=1"]
    fn fd_limit_stops_watches() {
        struct RestoreLimit(libc::rlimit);
        impl RestoreLimit {
            fn lower(soft_limit: libc::rlim_t) -> Self {
                let mut original = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                // SAFETY: original points to writable storage for the returned limit.
                assert_eq!(
                    unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut original) },
                    0
                );
                let lowered = libc::rlimit {
                    rlim_cur: original.rlim_cur.min(soft_limit),
                    ..original
                };
                // SAFETY: only the soft limit of this process is lowered.
                assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lowered) }, 0);
                Self(original)
            }
        }
        impl Drop for RestoreLimit {
            fn drop(&mut self) {
                // SAFETY: the saved limit was obtained from getrlimit for this process.
                assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &self.0) }, 0);
            }
        }

        for triggered_by_event in [false, true] {
            let dir = testdir();
            let root = dir.path();
            let (mut event_loop, rx) = recording_event_loop();
            if triggered_by_event {
                event_loop
                    .add_watch(WatchPath::new(root).unwrap(), true, true)
                    .unwrap();
            }
            for index in 0..128 {
                std::fs::write(root.join(index.to_string()), "").unwrap();
            }

            let restore = RestoreLimit::lower(64);
            let result = if triggered_by_event {
                event_loop.handle_kqueue();
                Ok(())
            } else {
                event_loop.add_watch(WatchPath::new(root).unwrap(), true, true)
            };
            drop(restore);

            let errors: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
            if triggered_by_event {
                assert_eq!(errors.len(), 1);
                assert!(matches!(errors[0].kind, ErrorKind::MaxFilesWatch));
            } else {
                assert!(matches!(result.unwrap_err().kind, ErrorKind::MaxFilesWatch));
                assert!(errors.is_empty());
            }
            assert!(event_loop.watches.contains_key(root));
            assert!(event_loop.watches.len() < 129);

            // A file added before exhaustion must have a live kernel watch, not just a map entry.
            let file = event_loop
                .watches
                .keys()
                .find(|path| path.as_path() != root)
                .unwrap()
                .clone();
            std::fs::write(&file, "changed").unwrap();
            event_loop.handle_kqueue();
            assert!(rx.try_iter().any(|event| {
                event.is_ok_and(|event| event.kind.is_modify() && event.paths.contains(&file))
            }));
        }

        let dir = testdir();
        let roots = [dir.path().join("first"), dir.path().join("second")];
        let (mut event_loop, rx) = recording_event_loop();
        for root in &roots {
            std::fs::create_dir(root).unwrap();
            event_loop
                .add_watch(WatchPath::new(root).unwrap(), true, true)
                .unwrap();
        }
        for root in &roots {
            std::fs::write(root.join("file"), "").unwrap();
        }

        // Keep both directory FDs alive, but make read_dir fail for both pending NOTE_WRITEs.
        // The event loop must report the batch's descriptor exhaustion only once.
        let restore = RestoreLimit::lower(0);
        event_loop.handle_kqueue();
        drop(restore);
        let errors: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0].kind, ErrorKind::MaxFilesWatch));
        assert_eq!(event_loop.watches.len(), roots.len());
    }

    #[test]
    fn test_remove_recursive() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let path = PathBuf::from("src");

        let mut watcher = KqueueWatcher::new(|event| println!("{event:?}"), Config::default())?;
        watcher.watch(&path, RecursiveMode::Recursive)?;
        let result = watcher.unwatch(&path);
        assert!(
            result.is_ok(),
            "unwatch yielded error: {}",
            result.unwrap_err()
        );
        Ok(())
    }

    #[test]
    fn recursive_watch_is_installed_before_directory_create_event_is_delivered() {
        let tmpdir = testdir();
        let root = tmpdir.path();
        let child = root.join("child");
        let file = child.join("created-from-handler.txt");

        let callback_child = child.clone();
        let callback_file = file.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = KqueueWatcher::new(
            move |event: Result<Event>| {
                if event.as_ref().is_ok_and(|event| {
                    matches!(event.kind, EventKind::Create(CreateKind::Folder))
                        && event.paths.contains(&callback_child)
                }) {
                    std::fs::write(&callback_file, "created by event handler")
                        .expect("create file from directory-create event handler");
                }
                let _ = tx.send(event);
            },
            Config::default(),
        )
        .expect("create watcher");
        watcher
            .watch(root, RecursiveMode::Recursive)
            .expect("watch recursively");

        std::fs::create_dir(&child).expect("create child directory");

        let mut rx = test::Receiver {
            rx,
            timeout: Duration::from_secs(5),
            detect_changes: None,
            kind: crate::WatcherKind::Kqueue,
        };
        rx.wait_unordered([
            expected(&child).create_folder(),
            expected(&file).create_file(),
        ]);
    }

    #[test]
    fn internal_recursive_refresh_preserves_explicit_child()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::create_dir(&child)?;

        let mut event_loop = test_event_loop()?;

        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;
        event_loop.add_watch(WatchPath::new(&child)?, false, true)?;

        event_loop.remove_watch(dir.path().to_path_buf(), false)?;
        assert!(
            event_loop
                .watches
                .get(&child)
                .is_some_and(|watch| watch.is_user_watch && !watch.user_is_recursive),
            "internal refresh removed explicit child watch"
        );

        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;

        let watched: HashMap<_, _> = event_loop
            .watches
            .iter()
            .filter(|(_path, watch)| watch.is_user_watch)
            .map(|(path, watch)| (path.clone(), watch.user_is_recursive))
            .collect();
        assert_eq!(watched.get(dir.path()), Some(&true));
        assert_eq!(watched.get(&child), Some(&false));

        Ok(())
    }

    #[test]
    fn write_watches_every_new_entry() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let mut event_loop = test_event_loop()?;
        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;

        // The NOTE_WRITE handler reports only the first new entry it finds as created. All new
        // entries and the contents of new subdirectories still need a watch.
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        let file = dir.path().join("file");
        std::fs::create_dir_all(first.join("nested"))?;
        std::fs::create_dir_all(second.join("nested"))?;
        std::fs::write(&file, "")?;
        event_loop.handle_kqueue();

        assert!(event_loop.watches.contains_key(&first.join("nested")));
        assert!(event_loop.watches.contains_key(&second.join("nested")));
        assert!(event_loop.watches.contains_key(&file));

        Ok(())
    }

    #[test]
    fn refresh_reports_cycle_and_registers_other_watches()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let (mut event_loop, rx) = recording_event_loop();
        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;

        let child = dir.path().join("child");
        let cycle = child.join("cycle");
        std::fs::create_dir(&child)?;
        std::os::unix::fs::symlink(".", &cycle)?;

        // The walk reports the cycle and continues installing the other watches.
        event_loop.handle_kqueue();
        let errors: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].paths, [cycle.clone()]);

        // Even that partial walk must register its watches to detect later changes.
        std::fs::remove_file(&cycle)?;
        let new = child.join("new");
        std::fs::write(&new, "")?;
        event_loop.handle_kqueue();

        assert!(event_loop.watches.contains_key(&new));

        Ok(())
    }

    // rmdir and mkdir of the same name can arrive in one batch. The scan for new entries has to
    // run after the NOTE_DELETE of the old subdirectory removed its watch.
    #[test]
    fn write_rewatches_recreated_subdir() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::create_dir(&child)?;
        let mut event_loop = test_event_loop()?;
        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;

        std::fs::remove_dir(&child)?;
        std::fs::create_dir(&child)?;
        event_loop.handle_kqueue();

        // A watch on the removed directory does not report entries created in the new one.
        let new = child.join("new");
        std::fs::write(&new, "")?;
        event_loop.handle_kqueue();

        assert!(event_loop.watches.contains_key(&new));

        Ok(())
    }

    #[test]
    fn write_watches_file_that_replaced_subdir()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::create_dir(&child)?;
        let mut event_loop = test_event_loop()?;
        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;

        std::fs::remove_dir(&child)?;
        std::fs::write(&child, "")?;
        event_loop.handle_kqueue();

        // The watch of the removed directory is recursive. A watch on the new file is not.
        let watch = event_loop.watches.get(&child).expect("child watch");
        assert!(!watch.is_recursive);

        Ok(())
    }

    #[test]
    fn link_change_keeps_non_recursive_watch() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let mut event_loop = test_event_loop()?;
        event_loop.add_watch(WatchPath::new(dir.path())?, false, true)?;

        let child = dir.path().join("child");
        std::fs::create_dir(&child)?;
        event_loop.handle_kqueue();

        assert!(!event_loop.watches.contains_key(&child));
        let watch = event_loop.watches.get(dir.path()).expect("root watch");
        assert!(!watch.is_recursive);
        assert!(!watch.user_is_recursive);

        Ok(())
    }

    #[test]
    fn recursive_remove_uses_tracked_watches() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::write(&child, "")?;

        let mut event_loop = test_event_loop()?;

        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;
        assert!(event_loop.watches.contains_key(&child));

        std::fs::remove_file(&child)?;
        event_loop.remove_watch(dir.path().to_path_buf(), false)?;

        assert!(!event_loop.watches.contains_key(&child));

        Ok(())
    }

    #[test]
    fn unreadable_child_is_reported_and_other_watches_are_registered()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        let locked = child.join("locked");
        std::fs::create_dir_all(&locked)?;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))?;
        let readable = std::fs::read_dir(&locked).is_ok();

        let (mut event_loop, rx) = recording_event_loop();
        let result = event_loop.add_watch(WatchPath::new(dir.path())?, true, true);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))?;
        // root can open the directory anyway, so the walk does not fail.
        if readable {
            return Ok(());
        }
        result?;
        let errors: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].paths, [locked]);
        assert!(
            matches!(&errors[0].kind, ErrorKind::Io(err) if err.kind() == io::ErrorKind::PermissionDenied)
        );

        let new = child.join("new");
        std::fs::create_dir(&new)?;
        event_loop.handle_kqueue();

        assert!(event_loop.watches.contains_key(&new));

        Ok(())
    }

    #[test]
    fn rewatching_same_path_replaces_recursive_state()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::create_dir(&child)?;

        let mut event_loop = test_event_loop()?;

        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;
        assert!(event_loop.watches.contains_key(&child));

        event_loop.add_watch(WatchPath::new(dir.path())?, false, true)?;

        let watch = event_loop.watches.get(dir.path()).expect("root watch");
        assert!(watch.is_user_watch);
        assert!(!watch.user_is_recursive);
        assert!(!watch.is_recursive);
        assert!(!event_loop.watches.contains_key(&child));

        Ok(())
    }

    #[test]
    fn rewatching_child_preserves_recursive_parent_state()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        let grandchild = child.join("grandchild");
        std::fs::create_dir_all(&grandchild)?;

        let (mut event_loop, rx) = recording_event_loop();

        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;
        event_loop.add_watch(WatchPath::new(&child)?, false, true)?;
        let cycle = child.join("cycle");
        std::os::unix::fs::symlink(".", &cycle)?;
        event_loop.add_watch(
            WatchPath::from_parts(child.clone(), PathBuf::from("reported-child")),
            false,
            true,
        )?;
        let errors: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].paths, [cycle]);

        let child_watch = event_loop.watches.get(&child).expect("child watch");
        assert!(child_watch.is_user_watch);
        assert!(!child_watch.user_is_recursive);
        assert!(child_watch.is_recursive);
        assert_eq!(child_watch.reported_path, PathBuf::from("reported-child"));

        let grandchild_watch = event_loop
            .watches
            .get(&grandchild)
            .expect("grandchild still covered by recursive parent");
        assert!(!grandchild_watch.is_user_watch);
        assert!(grandchild_watch.is_recursive);

        Ok(())
    }

    #[test]
    fn nested_recursive_watch_keeps_its_spelling_under_an_outer_watch()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        let grandchild = child.join("grandchild");
        std::fs::create_dir_all(&grandchild)?;

        let mut event_loop = test_event_loop()?;

        event_loop.add_watch(
            WatchPath::from_parts(child.clone(), PathBuf::from("reported-child")),
            true,
            true,
        )?;
        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;

        let reported = |event_loop: &EventLoop, path: &Path| {
            let watch = event_loop.watches.get(path).expect("watch");
            watch.reported_path.clone()
        };
        assert_eq!(reported(&event_loop, dir.path()), dir.path());
        assert_eq!(reported(&event_loop, &child), Path::new("reported-child"));
        assert_eq!(
            reported(&event_loop, &grandchild),
            Path::new("reported-child/grandchild")
        );

        event_loop.remove_watch(dir.path().to_path_buf(), false)?;

        assert_eq!(
            reported(&event_loop, &grandchild),
            Path::new("reported-child/grandchild")
        );

        Ok(())
    }

    #[test]
    fn rewatching_carved_out_child_does_not_restore_parent_recursive_state()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        let grandchild = child.join("grandchild");
        std::fs::create_dir_all(&grandchild)?;

        let mut event_loop = test_event_loop()?;

        event_loop.add_watch(WatchPath::new(dir.path())?, true, true)?;
        event_loop.remove_watch(child.clone(), false)?;
        event_loop.add_watch(WatchPath::new(&child)?, false, true)?;
        event_loop.add_watch(
            WatchPath::from_parts(child.clone(), PathBuf::from("reported-child")),
            false,
            true,
        )?;

        let child_watch = event_loop.watches.get(&child).expect("child watch");
        assert!(child_watch.is_user_watch);
        assert!(!child_watch.user_is_recursive);
        assert!(!child_watch.is_recursive);
        assert_eq!(child_watch.reported_path, PathBuf::from("reported-child"));
        assert!(!event_loop.watches.contains_key(&grandchild));

        Ok(())
    }

    #[test]
    fn create_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        rx.wait_unordered([expected(path).create_file()]);
    }

    #[test]
    fn write_file() {
        let tmpdir = testdir();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        let (mut watcher, mut rx) = watcher();

        watcher.watch_recursively(&tmpdir);

        std::fs::write(&path, b"123").expect("write");

        rx.wait_unordered([expected(&path).modify_data_any()]);
    }

    #[test]
    fn chmod_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let path = tmpdir.path().join("entry");
        let file = std::fs::File::create_new(&path).expect("create");
        let mut permissions = file.metadata().expect("metadata").permissions();
        permissions.set_readonly(true);

        watcher.watch_recursively(&tmpdir);
        file.set_permissions(permissions).expect("set_permissions");

        rx.wait_unordered([expected(&path).modify_meta_any()]);
    }

    #[test]
    fn rename_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        let new_path = tmpdir.path().join("renamed");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_unordered([expected(path).rename_any(), expected(new_path).create()]);
    }

    #[test]
    fn delete_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();
        let file = tmpdir.path().join("file");
        std::fs::write(&file, "").expect("write");

        watcher.watch_nonrecursively(&tmpdir);

        std::fs::remove_file(&file).expect("remove");

        // kqueue reports a write event on the directory when a file is deleted
        rx.wait_unordered([expected(tmpdir.path()).modify_data_any()]);
    }

    #[test]
    fn create_file_in_non_recursive_directory_with_existing_child() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();
        let existing = tmpdir.path().join("existing");
        let created = tmpdir.path().join("created");
        std::fs::write(&existing, "").expect("write");

        watcher.watch_nonrecursively(&tmpdir);

        std::fs::write(&created, "").expect("write");

        // kqueue does not report which directory entry changed, so the backend
        // must not guess an arbitrary pre-existing child as the created path.
        rx.wait_unordered([expected(tmpdir.path()).modify_data_any()]);
    }

    #[test]
    fn delete_self_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();
        let file = tmpdir.path().join("file");
        std::fs::write(&file, "").expect("write");

        watcher.watch_nonrecursively(&file);

        std::fs::remove_file(&file).expect("remove");

        rx.wait_unordered([expected(file).remove_any()]);
    }

    #[test]
    fn delete_self_dir() {
        let tmpdir = testdir();
        let dir = tmpdir.path().join("dir");
        std::fs::create_dir(&dir).expect("create");

        let (mut watcher, mut rx) = watcher();
        watcher.watch_nonrecursively(&dir);

        std::fs::remove_dir(&dir).expect("remove");

        rx.wait_unordered([expected(&dir).remove_any()]);
    }

    #[test]
    #[ignore = "non-recursive kqueue directory watches do not report individual child events"]
    fn create_write_overwrite() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();
        let overwritten_file = tmpdir.path().join("overwritten_file");
        let overwriting_file = tmpdir.path().join("overwriting_file");
        std::fs::write(&overwritten_file, "123").expect("write1");

        watcher.watch_nonrecursively(&tmpdir);

        std::fs::File::create(&overwriting_file).expect("create");
        std::fs::write(&overwriting_file, "321").expect("write2");
        std::fs::rename(&overwriting_file, &overwritten_file).expect("rename");

        rx.wait_unordered([
            expected(&overwriting_file).create_file(),
            expected(&overwriting_file).modify_data_any().multiple(),
            expected(&overwriting_file).rename_any(),
            expected(&overwritten_file).rename_any(),
        ])
        .ensure_no_tail();
    }

    #[test]
    fn create_dir() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create");

        rx.wait_unordered([expected(&path).create_folder()]);
    }

    #[test]
    fn chmod_dir() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");
        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_readonly(true);

        watcher.watch_recursively(&tmpdir);
        std::fs::set_permissions(&path, permissions).expect("set_permissions");

        rx.wait_unordered([expected(&path).modify_meta_any()]);
    }

    #[test]
    fn rename_dir() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let path = tmpdir.path().join("entry");
        let new_path = tmpdir.path().join("new_path");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_ordered([
            expected(&new_path).create_folder(),
            expected(&path).rename_any(),
        ]);
    }

    #[test]
    fn delete_dir() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::remove_dir(&path).expect("remove");

        rx.wait_unordered([expected(path).remove_any()]);
    }

    #[test]
    #[ignore = "kqueue directory scans can miss the intermediate path between rapid renames"]
    fn rename_dir_twice() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let path = tmpdir.path().join("entry");
        let new_path = tmpdir.path().join("new_path");
        let new_path2 = tmpdir.path().join("new_path2");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::rename(&path, &new_path).expect("rename");
        std::fs::rename(&new_path, &new_path2).expect("rename2");

        rx.wait_unordered([
            expected(&path).rename_any(),
            expected(&new_path).create_folder(),
            expected(&new_path).rename_any(),
            expected(&new_path2).create_folder(),
        ]);
    }

    #[test]
    fn move_out_of_watched_dir() {
        let tmpdir = testdir();
        let subdir = tmpdir.path().join("subdir");
        let (mut watcher, mut rx) = watcher();

        let path = subdir.join("entry");
        std::fs::create_dir_all(&subdir).expect("create_dir_all");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&subdir);
        let new_path = tmpdir.path().join("entry");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_unordered([expected(path).rename_any()]);
    }

    // kqueue can also miss files created and removed before child watches are installed.
    #[test]
    #[ignore = "expects inotify-style DataChange::Content and RemoveKind::File; kqueue emits Any"]
    fn create_write_write_rename_write_remove() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

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
            expected(&file1).create_file(),
            expected(&file1).modify_data_content(),
            expected(&file2).modify_data_content(),
            expected(&file1).rename_any(),
            expected(&new_path).rename_any(),
            expected(&new_path).modify_data_content(),
            expected(&new_path).remove_file(),
        ]);
    }

    #[test]
    #[ignore = "kqueue directory scans can miss the intermediate path between rapid renames"]
    fn rename_twice() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        let new_path1 = tmpdir.path().join("renamed1");
        let new_path2 = tmpdir.path().join("renamed2");

        std::fs::rename(&path, &new_path1).expect("rename1");
        std::fs::rename(&new_path1, &new_path2).expect("rename2");

        rx.wait_unordered([
            expected(&path).rename_any(),
            expected(&new_path1).rename_any(),
            expected(&new_path2).create(),
        ]);
    }

    #[test]
    fn set_file_mtime() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let path = tmpdir.path().join("entry");
        let file = std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);

        file.set_modified(
            std::time::SystemTime::now()
                .checked_sub(Duration::from_secs(60 * 60))
                .expect("time"),
        )
        .expect("set_time");

        rx.wait_unordered([expected(&path).modify_meta_any()]);
    }

    #[test]
    fn write_file_non_recursive_watch() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_nonrecursively(&path);

        std::fs::write(&path, b"123").expect("write");

        rx.wait_unordered([expected(path).modify_data_any()]);
    }

    #[test]
    fn write_to_a_hardlink_pointed_to_the_watched_file_triggers_an_event() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let subdir = tmpdir.path().join("subdir");
        let file = subdir.join("file");
        let hardlink = tmpdir.path().join("hardlink");

        std::fs::create_dir(&subdir).expect("create");
        std::fs::write(&file, "").expect("file");
        std::fs::hard_link(&file, &hardlink).expect("hardlink");

        watcher.watch_nonrecursively(&file);

        std::fs::write(&hardlink, "123123").expect("write to the hard link");

        rx.wait_unordered([expected(file).modify_data_any()]);
    }

    #[test]
    #[ignore = "kqueue does not emit create events for descendants found while adding child watches"]
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

        let (mut watcher, mut rx) = watcher();

        watcher.watch_recursively(&tmpdir);

        std::fs::create_dir_all(&nested9).expect("create_dir_all");

        rx.wait_unordered([
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
    }

    #[test]
    fn kqueue_watcher_respects_event_kind_mask() {
        use crate::Watcher;
        use notify_types::event::EventKindMask;

        let tmpdir = testdir();
        let (tx, rx) = std::sync::mpsc::channel();

        // Create watcher with CREATE-only mask (no MODIFY events)
        let config = Config::default().with_event_kinds(EventKindMask::CREATE);

        let mut watcher = KqueueWatcher::new(tx, config).expect("create watcher");
        watcher
            .watch(tmpdir.path(), crate::RecursiveMode::Recursive)
            .expect("watch");

        let path = tmpdir.path().join("test_file");

        // Create a file - should generate CREATE event
        std::fs::File::create_new(&path).expect("create");

        // Small delay to let events propagate
        std::thread::sleep(Duration::from_millis(100));

        // Modify the file - should NOT generate event (filtered by mask)
        std::fs::write(&path, "modified content").expect("write modified");

        std::thread::sleep(Duration::from_millis(100));

        // Collect all events
        let events: Vec<_> = rx.try_iter().filter_map(|r| r.ok()).collect();

        // Should have CREATE event
        assert!(
            events.iter().any(|e| e.kind.is_create()),
            "Expected CREATE event, got: {events:?}"
        );

        // Should NOT have MODIFY event (filtered out)
        assert!(
            !events.iter().any(|e| e.kind.is_modify()),
            "Should not receive MODIFY events with CREATE-only mask, got: {events:?}"
        );
    }
}
