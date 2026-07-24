//! Watcher implementation for the kqueue API
//!
//! The kqueue() system call provides a generic method of notifying the user
//! when an event happens or a condition holds, based on the results of small
//! pieces of kernel code termed filters.

use super::event::*;
use super::{
    Config, Error, ErrorKind, EventHandler, EventKindMask, RecursiveMode, Result, WatchFilter,
    Watcher,
};
use crate::paths::{
    absolute_path, check_watch_barriers, filter_allows_dir, filter_keeps_walk_entry,
    is_preserved_watch_root, preserved_watch_mode, preserved_watch_roots,
    recursive_user_watch_ancestor, reported_path, WatchMetadata as Watch, WatchPath,
};
use crate::{unbounded, Receiver, Sender};
use kqueue::{EventData, EventFilter, FilterFlag, Ident};
use std::collections::{HashMap, HashSet};
use std::fs::metadata;
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
    // Directories the filter excluded from watching. New-entry discovery treats "not watched"
    // as "new", so excluded directories must be remembered or they would be re-discovered
    // (and re-reported as created) on every write to their parent.
    seen_excluded: HashSet<PathBuf>,
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
    AddWatch(WatchPath, RecursiveMode, WatchFilter, Sender<Result<()>>),
    RemoveWatch(PathBuf, Sender<Result<()>>),
    GetWatchedPaths(Sender<Vec<(PathBuf, RecursiveMode)>>),
    Shutdown,
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
            seen_excluded: HashSet::new(),
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
                EventLoopMsg::AddWatch(path, recursive_mode, watch_filter, tx) => {
                    let _ = tx.send(self.add_watch(
                        path,
                        recursive_mode.is_recursive(),
                        true,
                        watch_filter,
                    ));
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
        let mut add_watches = Vec::new();
        let mut remove_watches = Vec::new();

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
                    let mut extra_events: Vec<Result<Event>> = Vec::new();
                    let event = match data {
                        /*
                        TODO: Differentiate folders and files
                        kqueue doesn't tell us if this was a file or a dir, so we
                        could only emulate this inotify behavior if we keep track of
                        all files and directories internally and then perform a
                        lookup.
                        */
                        kqueue::Vnode::Delete => {
                            remove_watches.push((path.clone(), true));
                            Ok(Event::new(EventKind::Remove(RemoveKind::Any)).add_path(event_path))
                        }

                        // A write to a recursively watched directory may mean that a new file
                        // was created in it. Non-recursive directory watches do not track
                        // children, so guessing from read_dir would be unreliable.
                        // FIXME: harden guessing for non-recursive watches.
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
                            // find which file is new in the directory by comparing it with our
                            // list of known watches (and remembered excluded directories)
                            match std::fs::read_dir(&path) {
                                Ok(dir) => {
                                    let watch_filter = watch
                                        .map(|watch| watch.watch_filter.clone())
                                        .unwrap_or_else(WatchFilter::accept_all);
                                    // Tombstoned excluded directories that vanished from this
                                    // directory never had their own watch to report their
                                    // deletion; synthesize the Remove event the other backends
                                    // deliver, and drop the tombstone so a re-creation is
                                    // reported again (this also covers replacement by a file,
                                    // which discovery below then announces as created).
                                    self.seen_excluded.retain(|p| {
                                        let still_excluded_dir = std::fs::symlink_metadata(p)
                                            .is_ok_and(|metadata| metadata.is_dir());
                                        if p.parent() != Some(path.as_path()) || still_excluded_dir
                                        {
                                            return true;
                                        }
                                        extra_events.push(Ok(Event::new(EventKind::Remove(
                                            RemoveKind::Folder,
                                        ))
                                        .add_path(reported_path(&path, &event_path, p))));
                                        false
                                    });
                                    let new_entry = dir
                                        .filter_map(std::result::Result::ok)
                                        .map(|f| f.path())
                                        .find(|f| {
                                            !self.watches.contains_key(f)
                                                && !self.seen_excluded.contains(f)
                                        });
                                    if let Some(file) = new_entry {
                                        let reported_file =
                                            reported_path(&path, &event_path, &file);
                                        let is_symlink = std::fs::symlink_metadata(&file)
                                            .map(|meta| meta.file_type().is_symlink())
                                            .unwrap_or(false);
                                        if file.is_dir()
                                            && !filter_allows_dir(&watch_filter, &file, is_symlink)
                                        {
                                            // The filter gates directories: an excluded
                                            // directory is reported as created but never
                                            // watched. Remember it so it is not "discovered"
                                            // again on every later write to this directory.
                                            self.seen_excluded.insert(file.clone());
                                        } else {
                                            // watch this new file
                                            add_watches.push((
                                                WatchPath::from_parts(
                                                    file.clone(),
                                                    reported_file.clone(),
                                                ),
                                                false,
                                                true,
                                                watch_filter,
                                            ));
                                        }

                                        Ok(Event::new(EventKind::Create(if file.is_dir() {
                                            CreateKind::Folder
                                        } else if file.is_file() {
                                            CreateKind::File
                                        } else {
                                            CreateKind::Other
                                        }))
                                        .add_path(reported_file))
                                    } else {
                                        Ok(Event::new(EventKind::Modify(ModifyKind::Data(
                                            DataChange::Any,
                                        )))
                                        .add_path(event_path))
                                    }
                                }
                                Err(e) => Err(e.into()),
                            }
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
                            // So for now, until we find a better solution, re-add the whole
                            // directory: the walk registers watches for entries that appeared
                            // (kqueue treats re-adding an existing kevent as an update, and
                            // deleted children fire their own NOTE_DELETE), merging into the
                            // existing entries. This is an expensive operation, as we recurse
                            // through all subdirectories.
                            //
                            // The re-add is a non-user merge: it must not disturb the entry's
                            // user metadata (requested mode and filter), so it re-walks with
                            // the entry's current merged mode and stored chain coverage and
                            // lets `WatchMetadata::new` preserve the user fields.
                            if let Some(watch) = watch {
                                add_watches.push((
                                    WatchPath::from_parts(path.clone(), event_path.clone()),
                                    false,
                                    watch.is_recursive,
                                    watch.watch_filter.clone(),
                                ));
                            }
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
                    for event in extra_events.into_iter().chain(std::iter::once(event)) {
                        match &event {
                            Ok(e) if !self.event_kinds.matches(&e.kind) => {
                                // Event filtered out
                            }
                            _ => self.event_handler.handle_event(event),
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

        for (path, is_user_watch, is_recursive, watch_filter) in add_watches {
            self.add_watch(path, is_recursive, is_user_watch, watch_filter)
                .ok();
        }
    }

    fn add_watch(
        &mut self,
        path: WatchPath,
        is_recursive: bool,
        is_user_watch: bool,
        watch_filter: WatchFilter,
    ) -> Result<()> {
        let path_is_dir = metadata(&path.absolute).map_err(Error::io)?.is_dir();
        let requested_is_recursive = is_recursive && path_is_dir;
        let mut inherited_recursive_root = None;
        if is_user_watch {
            check_watch_barriers(
                &path.absolute,
                &path.requested,
                path_is_dir,
                requested_is_recursive,
                &watch_filter,
                self.watches
                    .iter()
                    .filter(|(_, watch)| watch.is_user_watch)
                    .map(|(path, watch)| {
                        (
                            path,
                            watch.is_dir,
                            watch.user_is_recursive,
                            &watch.watch_filter,
                        )
                    }),
            )?;
            if let Some(watch) = self
                .watches
                .get(&path.absolute)
                .filter(|watch| watch.is_user_watch)
            {
                if watch.rewatch_is_noop(&path, requested_is_recursive, &watch_filter) {
                    return Ok(());
                }

                // Rewatching an explicit user watch replaces its requested mode, reported
                // path, and filter instead of merging with the previous metadata. If a
                // recursive ancestor also covers this directory, remember it so coverage
                // the removal drops (or the new mode no longer provides) is rebuilt below;
                // the overlap barrier guarantees such an ancestor is unfiltered.
                inherited_recursive_root =
                    if path_is_dir && !requested_is_recursive && watch.is_recursive {
                        recursive_user_watch_ancestor(&path.absolute, self.watches.iter())
                    } else {
                        None
                    };
                self.remove_watch(path.absolute.clone(), false)?;
            }
        }

        // If the watch is not recursive, or if we determine (by stat'ing the path to get
        // its metadata) that the watched path is not a directory, add a single path watch.
        if !requested_is_recursive {
            self.add_single_watch(path.clone(), false, is_user_watch, watch_filter)?;
        } else {
            let root = path.clone();
            let mut first = true;
            // Prune rejected directories manually: don't watch them and don't descend
            // into them (shielding the walk from errors inside excluded subtrees), and
            // remember them so runtime discovery doesn't treat them as new entries.
            let mut walk = WalkDir::new(&root.absolute)
                .follow_links(self.follow_symlinks)
                .into_iter();
            while let Some(entry) = walk.next() {
                let entry = entry.map_err(map_walkdir_error)?;
                if !filter_keeps_walk_entry(&watch_filter, &entry) {
                    let excluded = entry.into_path();
                    walk.skip_current_dir();
                    self.seen_excluded.insert(excluded);
                    continue;
                }
                // WalkDir yields the root first; only it is the user-requested watch.
                self.add_single_watch(
                    root.child(entry.into_path()),
                    is_recursive,
                    is_user_watch && first,
                    watch_filter.clone(),
                )?;
                first = false;
            }
        }

        if let Some((ancestor_path, ancestor_reported_path)) = inherited_recursive_root {
            // Re-add, as non-user watches, the coverage the covering ancestor provides but
            // the removal above dropped (or the new non-recursive mode no longer provides).
            for entry in WalkDir::new(&path.absolute).follow_links(self.follow_symlinks) {
                let absolute = match entry {
                    Ok(entry) => entry.into_path(),
                    Err(err) if walkdir_error_is_not_found(&err) => continue,
                    Err(err) => return Err(map_walkdir_error(err)),
                };
                let requested = reported_path(&ancestor_path, &ancestor_reported_path, &absolute);
                let result = self.add_single_watch(
                    WatchPath::from_parts(absolute, requested),
                    true,
                    false,
                    WatchFilter::accept_all(),
                );
                if let Err(err) = result {
                    if !error_is_not_found(&err) {
                        return Err(err);
                    }
                }
            }
        }

        // Only make a single `kevent` syscall to add all the watches.
        self.kqueue.watch()?;

        Ok(())
    }

    /// Adds a single watch to the kqueue.
    ///
    /// The caller of this function must call `self.kqueue.watch()` afterwards to register the new watch.
    fn add_single_watch(
        &mut self,
        path: WatchPath,
        is_recursive: bool,
        is_user_watch: bool,
        watch_filter: WatchFilter,
    ) -> Result<()> {
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
            .map_err(|e| Error::io(e).add_path(path.requested.clone()))?;
        let existing_watch = self.watches.get(&path.absolute);
        let watch = Watch::new(
            &path,
            path.absolute.is_dir(),
            is_recursive,
            is_user_watch,
            existing_watch,
            self.watches.iter(),
            watch_filter,
        );
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

                // Drop excluded-directory tombstones under the removed path. The overlap
                // barrier guarantees they belonged to this watch alone (no other watch may
                // overlap a filtered one). Runs only after a successful removal; a failed
                // unwatch must not disturb tombstones.
                self.seen_excluded.retain(|p| !p.starts_with(&path));

                self.kqueue.watch()?;
            }
        }
        Ok(())
    }
}

fn map_walkdir_error(e: walkdir::Error) -> Error {
    if e.io_error().is_some() {
        // save to unwrap otherwise we whouldn't be in this branch
        Error::io(e.into_io_error().unwrap())
    } else {
        Error::generic(&e.to_string())
    }
}

fn walkdir_error_is_not_found(e: &walkdir::Error) -> bool {
    e.io_error()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}

fn error_is_not_found(e: &Error) -> bool {
    matches!(&e.kind, ErrorKind::PathNotFound)
        || matches!(&e.kind, ErrorKind::Io(io_err) if io_err.kind() == std::io::ErrorKind::NotFound)
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

    fn watch_inner(
        &mut self,
        path: &Path,
        recursive_mode: RecursiveMode,
        watch_filter: WatchFilter,
    ) -> Result<()> {
        let pb = WatchPath::new(path)?;
        let (tx, rx) = unbounded();
        let msg = EventLoopMsg::AddWatch(pb, recursive_mode, watch_filter, tx);

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

    fn watch_filtered(
        &mut self,
        path: &Path,
        recursive_mode: RecursiveMode,
        watch_filter: WatchFilter,
    ) -> Result<()> {
        self.watch_inner(path, recursive_mode, watch_filter)
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
    fn watch_filter_prunes_excluded_directories(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let excluded = dir.path().join("excluded");
        let excluded_sub = excluded.join("sub");
        let included = dir.path().join("included");
        std::fs::create_dir_all(&excluded_sub)?;
        std::fs::create_dir_all(&included)?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::with_filter(|p: &Path| {
                p.file_name() != Some(std::ffi::OsStr::new("excluded"))
            }),
        )?;

        assert!(event_loop.watches.contains_key(dir.path()));
        assert!(event_loop.watches.contains_key(&included));
        assert!(!event_loop.watches.contains_key(&excluded));
        assert!(
            !event_loop.watches.contains_key(&excluded_sub),
            "descent into excluded directories must be pruned"
        );
        assert!(
            event_loop.seen_excluded.contains(&excluded),
            "the walk must seed tombstones so discovery doesn't re-report pre-existing \
             excluded directories as new"
        );
        assert!(
            !event_loop.seen_excluded.contains(&excluded_sub),
            "tombstones only cover the pruned root, not its unvisited contents"
        );

        Ok(())
    }

    #[test]
    fn nonrecursive_rewatch_preserves_ancestor_filter(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::create_dir_all(&child)?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::with_filter(|p: &Path| {
                p.file_name() != Some(std::ffi::OsStr::new("excluded"))
            }),
        )?;
        // Simulates a plain `watch(child, NonRecursive)`, which passes an accept-all filter.
        let result = event_loop.add_watch(
            WatchPath::new(&child)?,
            false,
            true,
            WatchFilter::accept_all(),
        );
        assert!(
            matches!(result, Err(ref e) if matches!(e.kind, ErrorKind::Generic(_))),
            "overlapping a filtered ancestor must fail, got {result:?}"
        );

        let stored = event_loop.watches.get(&child).expect("child watch");
        assert!(stored.is_recursive, "child keeps inherited coverage");
        assert!(
            !stored.watch_filter.should_watch(&child.join("excluded")),
            "the ancestor's filter must survive a non-recursive rewatch"
        );

        Ok(())
    }

    #[test]
    fn rewatch_with_different_filter_rebuilds_watch(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let excluded = dir.path().join("excluded");
        std::fs::create_dir_all(&excluded)?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::with_filter(|p: &Path| {
                p.file_name() != Some(std::ffi::OsStr::new("excluded"))
            }),
        )?;
        assert!(!event_loop.watches.contains_key(&excluded));

        // Re-watching with a different filter (here: accept-all) must rebuild the watch
        // instead of short-circuiting as unchanged.
        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::accept_all(),
        )?;
        assert!(
            event_loop.watches.contains_key(&excluded),
            "a rewatch with a different filter must take effect"
        );

        Ok(())
    }

    #[test]
    fn rejected_root_preserves_existing_watch(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::create_dir_all(&child)?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::accept_all(),
        )?;
        let before = event_loop
            .watches
            .keys()
            .cloned()
            .collect::<std::collections::HashSet<_>>();

        let rejecting = dir.path().to_path_buf();
        let result = event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::with_filter(move |p: &Path| p != rejecting.as_path()),
        );

        assert!(
            matches!(result, Err(ref e) if matches!(e.kind, ErrorKind::PathExcluded)),
            "watching a rejected root must fail with PathExcluded: {result:?}"
        );
        assert_eq!(
            event_loop
                .watches
                .keys()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
            before,
            "a failed rewatch must leave existing watches unchanged"
        );

        Ok(())
    }

    // A directory the filter excludes is reported (its watched parent sees the change) but
    // never watched, and is remembered as a tombstone so runtime discovery does not keep
    // re-reporting it. Prove both halves: the create surfaces, and nothing beneath it leaks.
    #[test]
    fn runtime_excluded_directory_is_reported_but_not_watched() {
        use crate::Watcher;
        use std::time::{Duration, Instant};

        let tmpdir = testdir();
        let root = tmpdir.path();

        let (mut watcher, mut rx) = watcher();
        watcher
            .watcher
            .watch_filtered(root, RecursiveMode::Recursive, reject_name("excluded"))
            .expect("watch filtered");

        let excluded = root.join("excluded");
        let included = root.join("included");

        // kqueue discovers one new directory entry per write notification, so create the
        // entries one at a time and wait for each discovery before creating the next.
        std::fs::create_dir(&excluded).expect("create excluded");
        rx.wait_unordered([expected(&excluded).create_folder()]);

        std::fs::create_dir(&included).expect("create included");
        rx.wait_unordered([expected(&included).create_folder()]);

        // The runtime watch on `included` is installed just after its create event is
        // emitted, so a single write could race ahead of the watch. Keep writing to both
        // directories until an event from `included` arrives, and assert nothing ever
        // leaks from inside the excluded (tombstoned, unwatched) directory.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_included_file = false;
        while !saw_included_file && Instant::now() < deadline {
            std::fs::write(excluded.join("hidden.txt"), "x").expect("write hidden");
            std::fs::write(included.join("seen.txt"), "x").expect("write seen");

            let attempt_deadline = Instant::now() + Duration::from_millis(200);
            while !saw_included_file && Instant::now() < attempt_deadline {
                if let Ok(Ok(event)) = rx.rx.recv_timeout(Duration::from_millis(50)) {
                    assert!(
                        !event
                            .paths
                            .iter()
                            .any(|p| p.starts_with(&excluded) && *p != excluded),
                        "event leaked from inside the excluded directory: {event:?}"
                    );
                    saw_included_file = event.paths.iter().any(|p| p.starts_with(&included));
                }
            }
        }
        assert!(
            saw_included_file,
            "expected an event from inside the included directory"
        );
    }

    // A pre-existing excluded directory is tombstoned by the initial walk and has no fd of
    // its own, so it cannot signal its own deletion. When it vanishes, the write to its
    // watched parent must make the backend synthesize the Remove event it could not emit.
    #[test]
    fn deleting_tombstoned_excluded_directory_reports_synthesized_remove() {
        use crate::Watcher;

        let tmpdir = testdir();
        let root = tmpdir.path();

        let excluded = root.join("excluded");
        std::fs::create_dir(&excluded).expect("create excluded");

        let (mut watcher, mut rx) = watcher();
        watcher
            .watcher
            .watch_filtered(root, RecursiveMode::Recursive, reject_name("excluded"))
            .expect("watch filtered");

        std::fs::remove_dir(&excluded).expect("remove excluded");
        rx.wait_unordered([expected(&excluded).remove_folder()]);
    }

    // Deleting a tombstoned excluded directory must also drop its tombstone, so that a later
    // re-creation is discovered and reported as new again rather than being swallowed as an
    // already-known entry. This is the observable proof that the tombstone drop happened.
    #[test]
    fn recreating_excluded_directory_after_deletion_is_reported_again() {
        use crate::Watcher;

        let tmpdir = testdir();
        let root = tmpdir.path();

        let excluded = root.join("excluded");
        std::fs::create_dir(&excluded).expect("create excluded");

        let (mut watcher, mut rx) = watcher();
        watcher
            .watcher
            .watch_filtered(root, RecursiveMode::Recursive, reject_name("excluded"))
            .expect("watch filtered");

        std::fs::remove_dir(&excluded).expect("remove excluded");
        rx.wait_unordered([expected(&excluded).remove_folder()]);

        std::fs::create_dir(&excluded).expect("recreate excluded");
        rx.wait_unordered([expected(&excluded).create_folder()]);
    }

    // Unwatching a root drops the tombstones seeded beneath it. The overlap barrier
    // guarantees those tombstones belonged to this watch alone, so none may survive it.
    #[test]
    fn unwatch_drops_excluded_tombstones() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let excluded = dir.path().join("excluded");
        std::fs::create_dir_all(&excluded)?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            reject_name("excluded"),
        )?;
        assert!(
            event_loop.seen_excluded.contains(&excluded),
            "the initial walk must seed a tombstone for the excluded directory"
        );

        // `false` mirrors the real `unwatch()` path; the watch's own recursive flag still
        // drives the recursive teardown that reaches the tombstone cleanup.
        event_loop.remove_watch(dir.path().to_path_buf(), false)?;
        assert!(
            !event_loop.seen_excluded.contains(&excluded),
            "unwatching the root must drop tombstones beneath it"
        );
        assert!(
            event_loop.seen_excluded.is_empty(),
            "no tombstone may outlive removal of its only watch: {:?}",
            event_loop.seen_excluded
        );

        Ok(())
    }

    #[test]
    fn internal_recursive_refresh_preserves_explicit_child(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::create_dir(&child)?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::accept_all(),
        )?;
        event_loop.add_watch(
            WatchPath::new(&child)?,
            false,
            true,
            WatchFilter::accept_all(),
        )?;

        event_loop.remove_watch(dir.path().to_path_buf(), false)?;
        assert!(
            event_loop
                .watches
                .get(&child)
                .is_some_and(|watch| watch.is_user_watch && !watch.user_is_recursive),
            "internal refresh removed explicit child watch"
        );

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::accept_all(),
        )?;

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
    fn recursive_remove_uses_tracked_watches() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::write(&child, "")?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::accept_all(),
        )?;
        assert!(event_loop.watches.contains_key(&child));

        std::fs::remove_file(&child)?;
        event_loop.remove_watch(dir.path().to_path_buf(), false)?;

        assert!(!event_loop.watches.contains_key(&child));

        Ok(())
    }

    #[test]
    fn rewatching_same_path_replaces_recursive_state(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        std::fs::create_dir(&child)?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::accept_all(),
        )?;
        assert!(event_loop.watches.contains_key(&child));

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            false,
            true,
            WatchFilter::accept_all(),
        )?;

        let watch = event_loop.watches.get(dir.path()).expect("root watch");
        assert!(watch.is_user_watch);
        assert!(!watch.user_is_recursive);
        assert!(!watch.is_recursive);
        assert!(!event_loop.watches.contains_key(&child));

        Ok(())
    }

    #[test]
    fn rewatching_child_preserves_recursive_parent_state(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        let grandchild = child.join("grandchild");
        std::fs::create_dir_all(&grandchild)?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::accept_all(),
        )?;
        event_loop.add_watch(
            WatchPath::new(&child)?,
            false,
            true,
            WatchFilter::accept_all(),
        )?;
        event_loop.add_watch(
            WatchPath::from_parts(child.clone(), PathBuf::from("reported-child")),
            false,
            true,
            WatchFilter::accept_all(),
        )?;

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
    fn rewatching_carved_out_child_does_not_restore_parent_recursive_state(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let child = dir.path().join("child");
        let grandchild = child.join("grandchild");
        std::fs::create_dir_all(&grandchild)?;

        let kqueue = kqueue::Watcher::new()?;
        let mut event_loop = EventLoop::new(kqueue, Box::new(|_| {}), false, EventKindMask::ALL)?;

        event_loop.add_watch(
            WatchPath::new(dir.path())?,
            true,
            true,
            WatchFilter::accept_all(),
        )?;
        event_loop.remove_watch(child.clone(), false)?;
        event_loop.add_watch(
            WatchPath::new(&child)?,
            false,
            true,
            WatchFilter::accept_all(),
        )?;
        event_loop.add_watch(
            WatchPath::from_parts(child.clone(), PathBuf::from("reported-child")),
            false,
            true,
            WatchFilter::accept_all(),
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
    #[ignore = "FIXME"]
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
    #[ignore = "FIXME"]
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

    #[test]
    #[ignore = "FIXME"]
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
    #[ignore = "FIXME"]
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
    #[ignore = "FIXME"]
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
