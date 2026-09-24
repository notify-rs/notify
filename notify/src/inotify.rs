//! Watcher implementation for the inotify Linux API
//!
//! The inotify API provides a mechanism for monitoring filesystem events.  Inotify can be used to
//! monitor individual files, or to monitor directories.  When a directory is monitored, inotify
//! will return events for the directory itself, and for files inside the directory.

use super::event::*;
use super::{Config, Error, ErrorKind, EventHandler, RecursiveMode, Result, WatchMode, Watcher};
use crate::bimap::BiHashMap;
use crate::{BoundSender, Receiver, Sender, TargetMode, bounded, unbounded};
use inotify as inotify_sys;
use inotify_sys::{EventMask, Inotify, WatchDescriptor, WatchMask};
use rustc_hash::FxBuildHasher;
use std::collections::HashMap;
use std::env;
use std::fs::metadata;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use walkdir::WalkDir;

const INOTIFY: mio::Token = mio::Token(0);
const MESSAGE: mio::Token = mio::Token(1);

/// What the ancestors of a tracked path are watched for: only their entries coming and going.
const ENTRY_MASK: WatchMask = WatchMask::CREATE
    .union(WatchMask::DELETE)
    .union(WatchMask::MOVED_FROM)
    .union(WatchMask::MOVED_TO);
const FULL_MASK: WatchMask = ENTRY_MASK
    .union(WatchMask::ATTRIB)
    .union(WatchMask::OPEN)
    .union(WatchMask::CLOSE_WRITE)
    .union(WatchMask::MODIFY);
const SELF_MASK: WatchMask = WatchMask::DELETE_SELF.union(WatchMask::MOVE_SELF);

#[derive(Clone, Copy, Debug)]
struct WatchInfo {
    mask: WatchMask,
    is_dir: bool,
}

#[cfg(test)]
impl WatchInfo {
    fn entries_only(self) -> bool {
        !self.mask.contains(WatchMask::MODIFY)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RootState {
    Missing,
    File,
    Directory,
}

/// A path the user watches. With `TargetMode::TrackPath` the path is tracked through every
/// ancestor: the ones that exist are watched for their entries, so that the root is reported
/// removed when any of them goes away, and created and watched again when it is reachable again.
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
    inotify: Option<Inotify>,
    event_handler: Box<dyn EventHandler>,
    watches: HashMap<PathBuf, RootWatch, FxBuildHasher>,
    watch_handles: BiHashMap<WatchDescriptor, PathBuf, WatchInfo, FxBuildHasher>,
    /// How many tracked roots lie strictly below each path.
    ancestors: HashMap<PathBuf, usize, FxBuildHasher>,
    rename_event: Option<Event>,
    follow_links: bool,
}

/// Watcher implementation based on inotify
#[derive(Debug)]
pub struct INotifyWatcher {
    channel: Sender<EventLoopMsg>,
    waker: Arc<mio::Waker>,
}

enum EventLoopMsg {
    AddWatch(PathBuf, WatchMode, Sender<Result<()>>),
    RemoveWatch(PathBuf, Sender<Result<()>>),
    Shutdown,
    Configure(Config, BoundSender<Result<bool>>),
    #[cfg(test)]
    GetWatchHandles(BoundSender<Vec<(PathBuf, WatchInfo)>>),
}

#[inline]
fn add_watch_by_event(
    path: &PathBuf,
    is_file_without_hardlinks: bool,
    watches: &HashMap<PathBuf, RootWatch, FxBuildHasher>,
    add_watches: &mut Vec<(PathBuf, bool, bool)>,
) {
    if let Some(root) = watches.get(path) {
        add_watches.push((
            path.to_owned(),
            root.mode.recursive_mode.is_recursive(),
            is_file_without_hardlinks,
        ));
        return;
    }

    let Some(parent) = path.parent() else {
        return;
    };
    if let Some(root) = watches.get(parent) {
        add_watches.push((
            path.to_owned(),
            root.mode.recursive_mode.is_recursive(),
            is_file_without_hardlinks,
        ));
        return;
    }

    for ancestor in parent.ancestors().skip(1) {
        if let Some(root) = watches.get(ancestor)
            && root.mode.recursive_mode == RecursiveMode::Recursive
        {
            add_watches.push((path.to_owned(), true, is_file_without_hardlinks));
            return;
        }
    }
}

#[inline]
fn remove_watch_by_event(
    path: &PathBuf,
    watch_handles: &BiHashMap<WatchDescriptor, PathBuf, WatchInfo, FxBuildHasher>,
    remove_watches: &mut Vec<PathBuf>,
) {
    if watch_handles.contains_right(path) {
        remove_watches.push(path.to_owned());
    }
}

impl EventLoop {
    pub fn new(
        inotify: Inotify,
        event_handler: Box<dyn EventHandler>,
        follow_links: bool,
    ) -> Result<Self> {
        let (event_loop_tx, event_loop_rx) = unbounded::<EventLoopMsg>();
        let poll = mio::Poll::new()?;

        let event_loop_waker = Arc::new(mio::Waker::new(poll.registry(), MESSAGE)?);

        let inotify_fd = inotify.as_raw_fd();
        let mut evented_inotify = mio::unix::SourceFd(&inotify_fd);
        poll.registry()
            .register(&mut evented_inotify, INOTIFY, mio::Interest::READABLE)?;

        let event_loop = EventLoop {
            running: true,
            poll,
            event_loop_waker,
            event_loop_tx,
            event_loop_rx,
            inotify: Some(inotify),
            event_handler,
            watches: HashMap::default(),
            watch_handles: BiHashMap::default(),
            ancestors: HashMap::default(),
            rename_event: None,
            follow_links,
        };
        Ok(event_loop)
    }

    // Run the event loop.
    pub fn run(self) {
        let result = thread::Builder::new()
            .name("notify-rs inotify loop".to_string())
            .spawn(|| self.event_loop_thread());
        if let Err(e) = result {
            tracing::error!(?e, "failed to start inotify event loop thread");
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
            INOTIFY => {
                // inotify has something to tell us.
                self.handle_inotify();
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
                EventLoopMsg::RemoveWatch(path, tx) => {
                    let result = tx.send(self.remove_watch(path));
                    if let Err(e) = result {
                        tracing::error!(?e, "failed to send RemoveWatch result");
                    }
                }
                EventLoopMsg::Shutdown => {
                    let result = self.remove_all_watches();
                    if let Err(e) = result {
                        tracing::error!(?e, "failed to remove all watches on shutdown");
                    }
                    if let Some(inotify) = self.inotify.take() {
                        let result = inotify.close();
                        if let Err(e) = result {
                            tracing::error!(?e, "failed to close inotify instance on shutdown");
                        }
                    }
                    self.running = false;
                    break;
                }
                EventLoopMsg::Configure(config, tx) => {
                    Self::configure_raw_mode(config, &tx);
                }
                #[cfg(test)]
                EventLoopMsg::GetWatchHandles(tx) => {
                    let handles = self
                        .watch_handles
                        .iter()
                        .map(|(_, path, info)| (path.clone(), *info))
                        .collect();
                    tx.send(handles).unwrap();
                }
            }
        }
    }

    fn configure_raw_mode(_config: Config, tx: &BoundSender<Result<bool>>) {
        tx.send(Ok(false))
            .expect("configuration channel disconnected");
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

    /// `path` exists now: a root there is present, and a directory may lead to roots below it.
    fn note_present(
        watches: &mut HashMap<PathBuf, RootWatch, FxBuildHasher>,
        ancestors: &HashMap<PathBuf, usize, FxBuildHasher>,
        path: &Path,
        is_dir: bool,
        appeared: &mut Vec<PathBuf>,
    ) {
        if let Some(root) = watches.get_mut(path) {
            root.state = if is_dir {
                RootState::Directory
            } else {
                RootState::File
            };
        }
        if is_dir && ancestors.contains_key(path) {
            appeared.push(path.to_path_buf());
        }
    }

    #[expect(clippy::too_many_lines)]
    fn handle_inotify(&mut self) {
        let mut add_watches = Vec::new();
        let mut remove_watches = Vec::new();
        let mut remove_watches_no_syscall = Vec::new();
        let mut vanished = Vec::new();
        let mut appeared = Vec::new();

        if let Some(ref mut inotify) = self.inotify {
            let mut buffer = [0; 1024];
            // Read all buffers available.
            loop {
                match inotify.read_events(&mut buffer) {
                    Ok(events) => {
                        let mut num_events = 0;
                        for event in events {
                            tracing::trace!(?event, "inotify event received");

                            num_events += 1;
                            if event.mask.contains(EventMask::Q_OVERFLOW) {
                                let ev = Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan));
                                self.event_handler.handle_event(ev);
                            }

                            let path = match event.name {
                                Some(name) => self
                                    .watch_handles
                                    .get_by_left(&event.wd)
                                    .map(|(root, _)| root.join(name)),
                                None => self
                                    .watch_handles
                                    .get_by_left(&event.wd)
                                    .map(|(root, _)| root.clone()),
                            };

                            let Some(path) = path else {
                                tracing::debug!(?event, "inotify event with unknown descriptor");
                                continue;
                            };

                            let mut evs = Vec::new();

                            if event.mask.contains(EventMask::MOVED_FROM) {
                                remove_watch_by_event(
                                    &path,
                                    &self.watch_handles,
                                    &mut remove_watches,
                                );

                                let event = Event::new(EventKind::Modify(ModifyKind::Name(
                                    RenameMode::From,
                                )))
                                .add_path(path.clone())
                                .set_tracker(event.cookie as usize);

                                self.rename_event = Some(event.clone());

                                if Self::is_watched_path(&self.watches, &path) {
                                    evs.push(event);
                                }
                                Self::note_gone(
                                    &mut self.watches,
                                    &self.ancestors,
                                    &path,
                                    &mut vanished,
                                );
                            } else if event.mask.contains(EventMask::MOVED_TO) {
                                if Self::is_watched_path(&self.watches, &path) {
                                    evs.push(
                                        Event::new(EventKind::Modify(ModifyKind::Name(
                                            RenameMode::To,
                                        )))
                                        .set_tracker(event.cookie as usize)
                                        .add_path(path.clone()),
                                    );

                                    let trackers_match =
                                        self.rename_event.as_ref().and_then(|e| e.tracker())
                                            == Some(event.cookie as usize);

                                    if trackers_match {
                                        let rename_event = self.rename_event.take().unwrap(); // unwrap is safe because `rename_event` must be set at this point
                                        let from_path = rename_event.paths.first();
                                        if from_path.is_none_or(|from_path| {
                                            Self::is_watched_path(&self.watches, from_path)
                                        }) {
                                            evs.push(
                                                Event::new(EventKind::Modify(ModifyKind::Name(
                                                    RenameMode::Both,
                                                )))
                                                .set_tracker(event.cookie as usize)
                                                .add_some_path(from_path.cloned())
                                                .add_path(path.clone()),
                                            );
                                        }
                                    }
                                }

                                let is_file_without_hardlinks = !event
                                    .mask
                                    .contains(EventMask::ISDIR)
                                    && metadata(&path).is_ok_and(|m| m.is_file_without_hardlinks());
                                add_watch_by_event(
                                    &path,
                                    is_file_without_hardlinks,
                                    &self.watches,
                                    &mut add_watches,
                                );
                                Self::note_present(
                                    &mut self.watches,
                                    &self.ancestors,
                                    &path,
                                    event.mask.contains(EventMask::ISDIR),
                                    &mut appeared,
                                );
                            }
                            if event.mask.contains(EventMask::MOVE_SELF) {
                                remove_watch_by_event(
                                    &path,
                                    &self.watch_handles,
                                    &mut remove_watches,
                                );
                                if let Some(root) = self.watches.get_mut(&path) {
                                    root.state = RootState::Missing;
                                }
                                if Self::is_watched_path(&self.watches, &path) {
                                    evs.push(
                                        Event::new(EventKind::Modify(ModifyKind::Name(
                                            RenameMode::From,
                                        )))
                                        .add_path(path.clone()),
                                    );
                                    // TODO stat the path and get to new path
                                    // - emit To and Both events
                                    // - change prefix for further events
                                }
                            }
                            if event.mask.contains(EventMask::CREATE) {
                                let is_dir = event.mask.contains(EventMask::ISDIR);
                                if Self::is_watched_path(&self.watches, &path) {
                                    evs.push(
                                        Event::new(EventKind::Create(if is_dir {
                                            CreateKind::Folder
                                        } else {
                                            CreateKind::File
                                        }))
                                        .add_path(path.clone()),
                                    );
                                }
                                let is_file_without_hardlinks = !is_dir
                                    && metadata(&path).is_ok_and(|m| m.is_file_without_hardlinks());
                                add_watch_by_event(
                                    &path,
                                    is_file_without_hardlinks,
                                    &self.watches,
                                    &mut add_watches,
                                );
                                Self::note_present(
                                    &mut self.watches,
                                    &self.ancestors,
                                    &path,
                                    is_dir,
                                    &mut appeared,
                                );
                            }
                            if event.mask.contains(EventMask::DELETE) {
                                if Self::is_watched_path(&self.watches, &path) {
                                    evs.push(
                                        Event::new(EventKind::Remove(
                                            if event.mask.contains(EventMask::ISDIR) {
                                                RemoveKind::Folder
                                            } else {
                                                RemoveKind::File
                                            },
                                        ))
                                        .add_path(path.clone()),
                                    );
                                }
                                remove_watch_by_event(
                                    &path,
                                    &self.watch_handles,
                                    &mut remove_watches,
                                );
                                Self::note_gone(
                                    &mut self.watches,
                                    &self.ancestors,
                                    &path,
                                    &mut vanished,
                                );
                            }
                            if event.mask.contains(EventMask::DELETE_SELF) {
                                let remove_kind = match self.watch_handles.get_by_right(&path) {
                                    Some((_, info)) if info.is_dir => RemoveKind::Folder,
                                    Some(_) => RemoveKind::File,
                                    None => RemoveKind::Other,
                                };
                                if let Some(root) = self.watches.get_mut(&path) {
                                    root.state = RootState::Missing;
                                }
                                if Self::is_watched_path(&self.watches, &path) {
                                    evs.push(
                                        Event::new(EventKind::Remove(remove_kind))
                                            .add_path(path.clone()),
                                    );
                                }
                                remove_watch_by_event(
                                    &path,
                                    &self.watch_handles,
                                    &mut remove_watches,
                                );
                            }
                            if event.mask.contains(EventMask::UNMOUNT) {
                                if Self::is_watched_path(&self.watches, &path) {
                                    evs.push(
                                        Event::new(EventKind::Remove(RemoveKind::Other))
                                            .add_path(path.clone()),
                                    );
                                }
                                // The kernel has already removed this watch descriptor and will
                                // emit IGNORED; clean up internal state without inotify_rm_watch.
                                // ref. https://www.man7.org/linux/man-pages/man7/inotify.7.html
                                remove_watch_by_event(
                                    &path,
                                    &self.watch_handles,
                                    &mut remove_watches_no_syscall,
                                );
                            }
                            if event.mask.contains(EventMask::MODIFY)
                                && Self::is_watched_path(&self.watches, &path)
                            {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Data(
                                        DataChange::Any,
                                    )))
                                    .add_path(path.clone()),
                                );
                            }
                            if event.mask.contains(EventMask::CLOSE_WRITE)
                                && Self::is_watched_path(&self.watches, &path)
                            {
                                evs.push(
                                    Event::new(EventKind::Access(AccessKind::Close(
                                        AccessMode::Write,
                                    )))
                                    .add_path(path.clone()),
                                );
                            }
                            if event.mask.contains(EventMask::CLOSE_NOWRITE)
                                && Self::is_watched_path(&self.watches, &path)
                            {
                                evs.push(
                                    Event::new(EventKind::Access(AccessKind::Close(
                                        AccessMode::Read,
                                    )))
                                    .add_path(path.clone()),
                                );
                            }
                            if event.mask.contains(EventMask::ATTRIB)
                                && Self::is_watched_path(&self.watches, &path)
                            {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Metadata(
                                        MetadataKind::Any,
                                    )))
                                    .add_path(path.clone()),
                                );
                            }
                            if event.mask.contains(EventMask::OPEN)
                                && Self::is_watched_path(&self.watches, &path)
                            {
                                evs.push(
                                    Event::new(EventKind::Access(AccessKind::Open(
                                        AccessMode::Any,
                                    )))
                                    .add_path(path.clone()),
                                );
                            }

                            for ev in evs {
                                self.event_handler.handle_event(Ok(ev));
                            }
                        }

                        // All events read. Break out.
                        if num_events == 0 {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // No events read. Break out.
                        break;
                    }
                    Err(e) => {
                        self.event_handler.handle_event(Err(Error::io(e)));
                    }
                }
            }
        }

        tracing::trace!(
            ?add_watches,
            ?remove_watches,
            "processing inotify watch changes"
        );

        for path in remove_watches_no_syscall {
            if self
                .watches
                .get(&path)
                .is_some_and(|root| root.mode.target_mode == TargetMode::NoTrack)
            {
                self.watches.remove(&path);
            }
            self.remove_maybe_recursive_watch(&path, true, true).ok();
        }

        for path in remove_watches {
            if self
                .watches
                .get(&path)
                .is_some_and(|root| root.mode.target_mode == TargetMode::NoTrack)
            {
                self.watches.remove(&path);
            }
            self.remove_maybe_recursive_watch(&path, true, false).ok();
        }

        for path in vanished {
            self.vanish_below(&path);
        }

        for (path, is_recursive, is_file_without_hardlinks) in add_watches {
            if let Err(add_watch_error) =
                self.add_maybe_recursive_watch(path, is_recursive, is_file_without_hardlinks, false)
            {
                // The handler should be notified if we have reached the limit.
                // Otherwise, the user might expect that a recursive watch
                // is continuing to work correctly, but it's not.
                if let ErrorKind::MaxFilesWatch = add_watch_error.kind {
                    self.event_handler.handle_event(Err(add_watch_error));

                    // After that kind of a error we should stop adding watches,
                    // because the limit has already reached and all next calls
                    // will return us only the same error.
                    break;
                }
            }
        }

        for path in appeared {
            self.rearm_below(&path);
        }
    }

    /// The roots below `path` are out of reach: report the ones that were present, and drop the
    /// watches below, which sit on moved or deleted inodes.
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
                Err(error) => {
                    let stop = matches!(error.kind, ErrorKind::MaxFilesWatch);
                    self.event_handler.handle_event(Err(error));
                    if stop {
                        break;
                    }
                }
            }
        }
    }

    fn remove_handles_below(&mut self, path: &Path) {
        let Some(ref mut inotify) = self.inotify else {
            return;
        };
        let mut inotify_watches = inotify.watches();
        let handles: Vec<(WatchDescriptor, PathBuf)> = (&self.watch_handles)
            .into_iter()
            .filter(|(_, handle_path, _)| handle_path.starts_with(path))
            .map(|(w, handle_path, _)| (w.clone(), handle_path.clone()))
            .collect();
        for (w, handle_path) in handles {
            tracing::trace!(
                "removing inotify watch below a vanished path: {}",
                handle_path.display()
            );
            // The kernel has already dropped the watch of a deleted inode; a moved one is still there.
            if let Err(e) = inotify_watches.remove(w.clone()) {
                tracing::trace!(?e, "inotify watch was already gone");
            }
            self.watch_handles.remove_by_left(&w);
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn add_watch(&mut self, path: PathBuf, watch_mode: WatchMode) -> Result<()> {
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
                ?need_upgrade_to_recursive,
                ?need_to_track,
                "upgrading existing watch for path: {}",
                path.display()
            );
            if need_to_track {
                self.arm_chain(&path)?;
                self.track_ancestors(&path);
            }
            if need_upgrade_to_recursive && metadata(&path).map_err(Error::io)?.is_dir() {
                self.add_maybe_recursive_watch(path.clone(), true, false, true)?;
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
            meta.is_file_without_hardlinks(),
            true,
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

    /// Watches a tracked root: its ancestors, and the root itself if it exists. The parent reports
    /// the root, so the root only gets a watch of its own when it is a directory or a hardlink.
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
            meta.is_file_without_hardlinks(),
            false,
        )?;
        Ok(if meta.is_dir() {
            RootState::Directory
        } else {
            RootState::File
        })
    }

    /// Watches the ancestors of `root` that exist, from the top down, for their entries; the
    /// parent for everything. Returns whether the parent exists.
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
                self.add_single_watch(ancestor, false, false)?;
            } else if let Err(e) = self.add_watch_with_mask(ancestor.clone(), ENTRY_MASK, false) {
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
            if !self.watches.contains_key(ancestor) && !self.is_below_recursive_root(ancestor) {
                self.remove_maybe_recursive_watch(ancestor, false, false)
                    .ok();
            }
        }
    }

    fn is_below_recursive_root(&self, path: &Path) -> bool {
        self.watches.iter().any(|(root, watch)| {
            watch.mode.recursive_mode.is_recursive()
                && root.as_path() != path
                && path.starts_with(root)
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn add_maybe_recursive_watch(
        &mut self,
        path: PathBuf,
        is_recursive: bool,
        is_file_without_hardlinks: bool,
        mut watch_self: bool,
    ) -> Result<()> {
        if is_recursive {
            for entry in WalkDir::new(&path)
                .follow_links(self.follow_links)
                .into_iter()
                .filter_map(filter_dir)
            {
                self.add_single_watch(entry.into_path(), false, watch_self)?;
                watch_self = false;
            }
        } else {
            self.add_single_watch(path, is_file_without_hardlinks, watch_self)?;
        }
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn add_single_watch(
        &mut self,
        path: PathBuf,
        is_file_without_hardlinks: bool,
        watch_self: bool,
    ) -> Result<()> {
        let mask = if watch_self {
            FULL_MASK.union(SELF_MASK)
        } else {
            FULL_MASK
        };
        self.add_watch_with_mask(path, mask, is_file_without_hardlinks)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn add_watch_with_mask(
        &mut self,
        path: PathBuf,
        mask: WatchMask,
        is_file_without_hardlinks: bool,
    ) -> Result<()> {
        let existing = self
            .watch_handles
            .get_by_right(&path)
            .map(|(_, info)| info.mask);
        if existing.is_some_and(|existing| existing.contains(mask)) {
            tracing::trace!(
                "watch handle already exists and no need to upgrade: {}",
                path.display()
            );
            return Ok(());
        }

        if is_file_without_hardlinks
            && let Some(parent) = path.parent()
            && self
                .watch_handles
                .get_by_right(parent)
                .is_some_and(|(_, info)| info.mask.contains(FULL_MASK))
        {
            tracing::trace!(
                "parent dir watch handle already exists and is a file without hardlinks: {}",
                path.display()
            );
            return Ok(());
        }

        // inotify replaces the mask of a path that is watched already.
        let watchmask = existing.map_or(mask, |existing| existing.union(mask));

        if let Some(ref mut inotify) = self.inotify {
            tracing::trace!("adding inotify watch: {}", path.display());

            match inotify.watches().add(&path, watchmask) {
                Err(e) => {
                    Err(if e.raw_os_error() == Some(libc::ENOSPC) {
                        // do not report inotify limits as "no more space" on linux #266
                        Error::new(ErrorKind::MaxFilesWatch)
                    } else if e.kind() == std::io::ErrorKind::NotFound {
                        Error::new(ErrorKind::PathNotFound)
                    } else {
                        Error::io(e)
                    }
                    .add_path(path))
                }
                Ok(w) => {
                    let is_dir = metadata(&path).map_err(Error::io)?.is_dir();
                    self.watch_handles.insert(
                        w,
                        path,
                        WatchInfo {
                            mask: watchmask,
                            is_dir,
                        },
                    );
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn remove_watch(&mut self, path: PathBuf) -> Result<()> {
        let Some(root) = self.watches.remove(&path) else {
            return Err(Error::watch_not_found().add_path(path));
        };
        self.remove_maybe_recursive_watch(&path, root.mode.recursive_mode.is_recursive(), false)?;
        if root.mode.target_mode == TargetMode::TrackPath {
            self.untrack_ancestors(&path);
        }
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn remove_maybe_recursive_watch(
        &mut self,
        path: &Path,
        is_recursive: bool,
        without_os_call: bool,
    ) -> Result<()> {
        let Some(ref mut inotify) = self.inotify else {
            return Ok(());
        };
        let mut inotify_watches = inotify.watches();

        if let Some((handle, _)) = self.watch_handles.remove_by_right(path) {
            tracing::trace!("removing inotify watch: {}", path.display());

            if !without_os_call {
                inotify_watches
                    .remove(handle)
                    .map_err(|e| Error::io(e).add_path(path.to_path_buf()))?;
            }
        }

        if is_recursive {
            let mut remove_list = Vec::new();
            for (w, p, _) in &self.watch_handles {
                if p.starts_with(path) {
                    if !without_os_call {
                        inotify_watches
                            .remove(w.clone())
                            .map_err(|e| Error::io(e).add_path(p.into()))?;
                    }
                    remove_list.push(w.clone());
                }
            }
            for w in remove_list {
                self.watch_handles.remove_by_left(&w);
            }
        }
        Ok(())
    }

    fn remove_all_watches(&mut self) -> Result<()> {
        if let Some(ref mut inotify) = self.inotify {
            let mut inotify_watches = inotify.watches();
            for (w, p, _) in &self.watch_handles {
                inotify_watches
                    .remove(w.clone())
                    .map_err(|e| Error::io(e).add_path(p.into()))?;
            }
            self.watch_handles.clear();
            self.watches.clear();
            self.ancestors.clear();
        }
        Ok(())
    }
}

/// return `DirEntry` when it is a directory
fn filter_dir(e: walkdir::Result<walkdir::DirEntry>) -> Option<walkdir::DirEntry> {
    if let Ok(e) = e
        && e.file_type().is_dir()
    {
        return Some(e);
    }
    None
}

impl INotifyWatcher {
    fn from_event_handler(
        event_handler: Box<dyn EventHandler>,
        follow_links: bool,
    ) -> Result<Self> {
        let inotify = Inotify::init()?;
        let event_loop = EventLoop::new(inotify, event_handler, follow_links)?;
        let channel = event_loop.event_loop_tx.clone();
        let waker = Arc::clone(&event_loop.event_loop_waker);
        event_loop.run();
        Ok(INotifyWatcher { channel, waker })
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

        // we expect the event loop to live and reply => unwraps must not panic
        self.channel.send(msg).unwrap();
        self.waker.wake().unwrap();
        rx.recv().unwrap()
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

        // we expect the event loop to live and reply => unwraps must not panic
        self.channel.send(msg).unwrap();
        self.waker.wake().unwrap();
        rx.recv().unwrap()
    }
}

impl Watcher for INotifyWatcher {
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
    fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.unwatch_inner(path)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn configure(&mut self, config: Config) -> Result<bool> {
        let (tx, rx) = bounded(1);
        self.channel.send(EventLoopMsg::Configure(config, tx))?;
        self.waker.wake()?;
        rx.recv()?
    }

    fn kind() -> crate::WatcherKind {
        crate::WatcherKind::Inotify
    }

    /// The watches that report the watched paths, without the ancestors watched for their entries
    /// only; see [`INotifyWatcher::get_chain_handles`].
    #[cfg(test)]
    fn get_watch_handles(&self) -> std::collections::HashSet<std::path::PathBuf> {
        self.handles(|info| !info.entries_only())
    }
}

#[cfg(test)]
impl INotifyWatcher {
    /// The ancestors of tracked paths, watched for their entries only.
    fn get_chain_handles(&self) -> std::collections::HashSet<std::path::PathBuf> {
        self.handles(|info| info.entries_only())
    }

    fn handles(
        &self,
        keep: impl Fn(&WatchInfo) -> bool,
    ) -> std::collections::HashSet<std::path::PathBuf> {
        let (tx, rx) = bounded(1);
        self.channel
            .send(EventLoopMsg::GetWatchHandles(tx))
            .unwrap();
        self.waker.wake().unwrap();
        rx.recv()
            .unwrap()
            .into_iter()
            .filter(|(_, info)| keep(info))
            .map(|(path, _)| path)
            .collect()
    }
}

impl Drop for INotifyWatcher {
    fn drop(&mut self) {
        // we expect the event loop to live => unwrap must not panic
        self.channel.send(EventLoopMsg::Shutdown).unwrap();
        self.waker.wake().unwrap();
    }
}

trait MetadataNotifyExt {
    fn is_file_without_hardlinks(&self) -> bool;
}

impl MetadataNotifyExt for std::fs::Metadata {
    #[inline]
    fn is_file_without_hardlinks(&self) -> bool {
        self.is_file() && self.nlink() == 1
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        path::{Path, PathBuf},
        sync::{Arc, atomic::AtomicBool, mpsc},
        thread::{self, available_parallelism},
        time::Duration,
    };

    use super::{Config, Error, ErrorKind, Event, INotifyWatcher, Result, Watcher};

    use crate::{
        RecursiveMode, TargetMode,
        config::WatchMode,
        event::{EventKind, ModifyKind},
        test::*,
    };

    fn watcher() -> (TestWatcher<INotifyWatcher>, Receiver) {
        channel()
    }

    #[test]
    fn inotify_watcher_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<INotifyWatcher>();
    }

    #[test]
    fn native_error_type_on_missing_path() {
        let mut watcher = INotifyWatcher::new(|_| {}, Config::default()).unwrap();

        let result = watcher.watch(
            &PathBuf::from("/some/non/existant/path"),
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

        // a tracked path is waited for, however deep the missing part
        watcher
            .watch(
                &PathBuf::from("/some/non/existant/path"),
                WatchMode::non_recursive(),
            )
            .unwrap();
    }

    /// Runs manually.
    ///
    /// * Save actual value of the limit: `MAX_USER_WATCHES=$(sysctl -n fs.inotify.max_user_watches)`
    /// * Run the test.
    /// * Set the limit to 0: `sudo sysctl fs.inotify.max_user_watches=0` while test is running
    /// * Wait for the test to complete
    /// * Restore the limit `sudo sysctl fs.inotify.max_user_watches=$MAX_USER_WATCHES`
    #[test]
    #[ignore = "requires changing sysctl fs.inotify.max_user_watches while test is running"]
    fn recursive_watch_calls_handler_if_creating_a_file_raises_max_files_watch() {
        use std::time::Duration;

        let tmpdir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (proc_changed_tx, proc_changed_rx) = std::sync::mpsc::channel();
        let proc_path = Path::new("/proc/sys/fs/inotify/max_user_watches");
        let mut watcher = INotifyWatcher::new(
            move |result: Result<Event>| match result {
                Ok(event) => {
                    if event.paths.first().is_some_and(|path| path == proc_path) {
                        proc_changed_tx.send(()).unwrap();
                    }
                }
                Err(e) => tx.send(e).unwrap(),
            },
            Config::default(),
        )
        .unwrap();

        watcher
            .watch(tmpdir.path(), WatchMode::recursive())
            .unwrap();
        watcher
            .watch(proc_path, WatchMode::non_recursive())
            .unwrap();

        // give the time to set the limit
        proc_changed_rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap();

        let child_dir = tmpdir.path().join("child");
        std::fs::create_dir(child_dir).unwrap();

        let result = rx.recv_timeout(Duration::from_millis(500));

        assert!(
            matches!(
                &result,
                Ok(Error {
                    kind: ErrorKind::MaxFilesWatch,
                    paths: _,
                })
            ),
            "expected {:?}, found: {:#?}",
            ErrorKind::MaxFilesWatch,
            result
        );
    }

    /// https://github.com/notify-rs/notify/issues/678
    #[test]
    fn race_condition_on_unwatch_and_pending_events_with_deleted_descriptor() {
        let tmpdir = tempfile::tempdir().expect("tmpdir");
        let (tx, rx) = mpsc::channel();
        let mut inotify = INotifyWatcher::new(
            move |e: Result<Event>| {
                let e = match e {
                    Ok(e) if e.paths.is_empty() => e,
                    Ok(_) | Err(_) => return,
                };
                let _ = tx.send(e);
            },
            Config::default(),
        )
        .expect("inotify creation");

        let dir_path = tmpdir.path();
        let file_path = dir_path.join("foo");
        std::fs::File::create(&file_path).unwrap();

        let stop = Arc::new(AtomicBool::new(false));

        let handles: Vec<_> = (0..available_parallelism().unwrap().get().max(4))
            .map(|_| {
                let file_path = file_path.clone();
                let stop = Arc::clone(&stop);
                thread::spawn(move || {
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = std::fs::File::open(&file_path).unwrap();
                    }
                })
            })
            .collect();

        let non_recursive = WatchMode::non_recursive();
        for _ in 0..(handles.len() * 4) {
            inotify.watch(dir_path, non_recursive).unwrap();
            inotify.unwatch(dir_path).unwrap();
        }

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for handle in handles {
            handle.join().ok().unwrap_or_default();
        }

        drop(inotify);

        let events: Vec<_> = rx.into_iter().map(|e| format!("{e:?}")).collect();

        const LOG_LEN: usize = 10;
        let events_len = events.len();
        assert!(
            events.is_empty(),
            "expected no events without path, but got {events_len}. first 10: {:#?}",
            &events[..LOG_LEN.min(events_len)]
        );
    }

    #[test]
    fn create_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).create_file(),
            expected(&path).access_open_any(),
            expected(&path).access_close_write(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
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
            expected(&path).create_file(),
            expected(&path).access_open_any(),
            expected(&path).access_close_write(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
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
        assert!(matches!(
            result,
            Err(Error {
                paths: _,
                kind: ErrorKind::PathNotFound
            })
        ));
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

        // The parent is watched once its creation is seen; the file may exist by then, in which
        // case the watcher reports it itself and the open and close are not seen.
        rx.wait_ordered([expected(&path).create_file()]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.path().join("entry")])
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
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([lib.clone(), lib.join("sub")])
        );
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
        rx.wait_unordered_exact([expected(&a).remove_file(), expected(&b).remove_file()]);
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));

        std::fs::rename(&moved, &lib).expect("rename back");
        rx.wait_unordered_exact([expected(&a).create_file(), expected(&b).create_file()]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([lib.clone(), lib.join("sub")])
        );

        std::fs::write(&a, "2").expect("write");
        rx.wait_ordered([expected(&a).modify_data_any()]);
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
        rx.wait_unordered([expected(&child).remove_folder()]);
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));

        std::fs::create_dir_all(&child).expect("create_dir_all");
        rx.wait_unordered([expected(&child).create_folder()]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([parent, child.clone()])
        );

        std::fs::File::create_new(child.join("file")).expect("create");
        rx.wait_ordered([expected(child.join("file")).create_file()]);
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
    fn create_file_nested_in_recursive_watch() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let nested1_dir = tmpdir.path().join("nested1");
        let nested2_dir = nested1_dir.join("nested2");
        std::fs::create_dir_all(&nested2_dir).expect("create_dir");

        watcher.watch_recursively(&tmpdir);

        let path = nested2_dir.join("entry");
        std::fs::File::create_new(&path).expect("create");

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&nested1_dir).access_open_any().optional(),
            expected(&nested2_dir).access_open_any().optional(),
            expected(&path).create_file(),
            expected(&path).access_open_any(),
            expected(&path).access_close_write(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([
                tmpdir.parent_path_buf(),
                tmpdir.to_path_buf(),
                nested1_dir,
                nested2_dir
            ])
        );
    }

    #[test]
    fn write_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        std::fs::write(&path, b"123").expect("write");

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).access_open_any(),
            expected(&path).modify_data_any().multiple(),
            expected(&path).access_close_write(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
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

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).modify_meta_any(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
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
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).rename_from(),
            expected(&new_path).rename_to(),
            expected([path, new_path]).rename_both(),
        ])
        .ensure_trackers_len(1)
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn rename_self_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_nonrecursively(&path);
        let new_path = tmpdir.path().join("renamed");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_ordered_exact([expected(&path).rename_from()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );

        std::fs::rename(&new_path, &path).expect("rename2");

        rx.wait_ordered_exact([expected(&path).rename_to()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
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

        rx.wait_ordered_exact([expected(&path).rename_from()])
            .ensure_no_tail();
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));

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
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&file).remove_file(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
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

        rx.wait_ordered_exact([expected(&file).remove_file()]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );

        std::fs::write(&file, "").expect("write");

        rx.wait_ordered_exact([expected(&file).create_file()]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
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
            expected(&file).modify_meta_any(),
            expected(&file).remove_file(),
        ]);
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

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&overwriting_file).create_file(),
            expected(&overwriting_file).access_open_any(),
            expected(&overwriting_file).access_close_write(),
            expected(&overwriting_file).access_open_any(),
            expected(&overwriting_file).modify_data_any().multiple(),
            expected(&overwriting_file).access_close_write().multiple(),
            expected(&overwriting_file).rename_from(),
            expected(&overwritten_file).rename_to(),
            expected([&overwriting_file, &overwritten_file]).rename_both(),
        ])
        .ensure_no_tail()
        .ensure_trackers_len(1);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn create_self_write_overwrite() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let overwritten_file = tmpdir.path().join("overwritten_file");
        let overwriting_file = tmpdir.path().join("overwriting_file");
        std::fs::write(&overwritten_file, "123").expect("write1");

        watcher.watch_nonrecursively(&overwritten_file);

        std::fs::File::create(&overwriting_file).expect("create");
        std::fs::write(&overwriting_file, "321").expect("write2");
        std::fs::rename(&overwriting_file, &overwritten_file).expect("rename");

        rx.wait_ordered_exact([expected(&overwritten_file).rename_to()])
            .ensure_no_tail()
            .ensure_trackers_len(1);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
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
        let mut expected_handles = HashSet::from([
            tmpdir.parent_path_buf(),
            tmpdir.to_path_buf(),
            nested_dir.clone(),
        ]);
        if upgrade_from_no_track {
            expected_handles.insert(watched_file.clone());
        }
        assert_eq!(watcher.get_watch_handles(), expected_handles);

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
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), nested_dir,])
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
    fn create_self_write_overwrite_no_track() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let overwritten_file = tmpdir.path().join("overwritten_file");
        let overwriting_file = tmpdir.path().join("overwriting_file");
        std::fs::write(&overwritten_file, "123").expect("write1");

        watcher.watch(
            &overwritten_file,
            WatchMode {
                recursive_mode: RecursiveMode::NonRecursive,
                target_mode: TargetMode::NoTrack,
            },
        );

        std::fs::File::create(&overwriting_file).expect("create");
        std::fs::write(&overwriting_file, "321").expect("write2");
        std::fs::rename(&overwriting_file, &overwritten_file).expect("rename");

        rx.wait_ordered_exact([
            expected(&overwritten_file).modify_meta_any(),
            expected(&overwritten_file).remove_file(),
        ])
        .ensure_no_tail()
        .ensure_trackers_len(0);
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));
    }

    #[test]
    fn create_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create");

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).create_folder(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), path])
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

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).access_open_any().optional(),
            expected(&path).modify_meta_any(),
            expected(&path).modify_meta_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), path])
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
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).access_open_any().optional(),
            expected(&path).rename_from(),
            expected(&new_path).rename_to(),
            expected([&path, &new_path]).rename_both(),
        ])
        .ensure_trackers_len(1);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), new_path])
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
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).access_open_any().optional(),
            expected(&path).remove_folder(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
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
            expected(&path).access_open_any().optional(),
            expected(&path).remove_folder(),
            expected(&path).access_open_any().optional(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );

        std::fs::create_dir(&path).expect("create_dir2");

        rx.wait_ordered_exact([
            expected(&path).access_open_any().optional(),
            expected(&path).create_folder(),
            expected(&path).access_open_any().optional(),
        ])
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

        rx.wait_ordered_exact([expected(&path).remove_folder()])
            .ensure_no_tail();
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));

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

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).access_open_any().optional(),
            expected(&path).rename_from(),
            expected(&new_path).rename_to(),
            expected([&path, &new_path]).rename_both(),
            expected(&new_path).access_open_any().optional(),
            expected(&new_path).rename_from(),
            expected(&new_path2).rename_to(),
            expected([&new_path, &new_path2]).rename_both(),
        ])
        .ensure_trackers_len(2);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), new_path2])
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
            expected(&subdir).access_open_any(),
            expected(&path).rename_from(),
        ])
        .ensure_trackers_len(1)
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

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&file1).create_file(),
            expected(&file1).access_open_any(),
            expected(&file1).modify_data_any().multiple(),
            expected(&file1).access_close_write(),
            expected(&file2).access_open_any(),
            expected(&file2).modify_data_any().multiple(),
            expected(&file2).access_close_write(),
            expected(&file1).access_open_any().optional(),
            expected(&file1).rename_from(),
            expected(&new_path).rename_to(),
            expected([&file1, &new_path]).rename_both(),
            expected(&new_path).access_open_any(),
            expected(&new_path).modify_data_any().multiple(),
            expected(&new_path).access_close_write(),
            expected(&new_path).remove_file(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
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

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).access_open_any().optional(),
            expected(&path).rename_from(),
            expected(&new_path1).rename_to(),
            expected([&path, &new_path1]).rename_both(),
            expected(&new_path1).access_open_any().optional(),
            expected(&new_path1).rename_from(),
            expected(&new_path2).rename_to(),
            expected([&new_path1, &new_path2]).rename_both(),
        ])
        .ensure_no_tail()
        .ensure_trackers_len(2);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
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

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&path).modify_data_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
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
            expected(&path).access_open_any(),
            expected(&path).modify_data_any().multiple(),
            expected(&path).access_close_write(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn watch_recursively_then_unwatch_child_stops_events_from_child() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let subdir = tmpdir.path().join("subdir");
        let file = subdir.join("file");
        std::fs::create_dir(&subdir).expect("create");

        watcher.watch_recursively(&tmpdir);

        std::fs::File::create(&file).expect("create");

        rx.wait_ordered_exact([
            expected(tmpdir.path()).access_open_any().optional(),
            expected(&subdir).access_open_any().optional(),
            expected(&file).create_file(),
            expected(&file).access_open_any(),
            expected(&file).access_close_write(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf(), subdir])
        );

        // TODO: https://github.com/rolldown/notify/issues/8
        // watcher.watcher.unwatch(&subdir).expect("unwatch");

        // std::fs::write(&file, b"123").expect("write");

        // std::fs::remove_dir_all(&subdir).expect("remove_dir_all");

        // rx.wait_ordered_exact([
        //     expected(&subdir).access_open_any().optional(),
        //     expected(&subdir).remove_folder(),
        // ])
        // .ensure_no_tail();
    }

    #[test]
    fn write_to_a_hardlink_pointed_to_the_watched_file_triggers_an_event() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let subdir = tmpdir.path().join("subdir");
        let subdir2 = tmpdir.path().join("subdir2");
        let file = subdir.join("file");
        let hardlink = subdir2.join("hardlink");

        std::fs::create_dir(&subdir).expect("create");
        std::fs::create_dir(&subdir2).expect("create2");
        std::fs::write(&file, "").expect("file");
        std::fs::hard_link(&file, &hardlink).expect("hardlink");

        watcher.watch_nonrecursively(&file);

        std::fs::write(&hardlink, "123123").expect("write to the hard link");

        rx.wait_ordered_exact([
            expected(&file).access_open_any(),
            expected(&file).modify_data_any().multiple(),
            expected(&file).access_close_write(),
        ]);
        assert_eq!(watcher.get_watch_handles(), HashSet::from([subdir, file]));
    }

    #[test]
    fn write_to_a_hardlink_pointed_to_the_watched_file_triggers_an_event_even_if_the_parent_is_watched()
     {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let subdir1 = tmpdir.path().join("subdir1");
        let subdir2 = subdir1.join("subdir2");
        let file = subdir2.join("file");
        let hardlink = tmpdir.path().join("hardlink");

        std::fs::create_dir_all(&subdir2).expect("create");
        std::fs::write(&file, "").expect("file");
        std::fs::hard_link(&file, &hardlink).expect("hardlink");

        watcher.watch_nonrecursively(&subdir2);
        watcher.watch_nonrecursively(&file);

        std::fs::write(&hardlink, "123123").expect("write to the hard link");

        rx.wait_ordered_exact([
            expected(&subdir2).access_open_any().optional(),
            expected(&file).access_open_any(),
            expected(&file).modify_data_any().multiple(),
            expected(&file).access_close_write(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([subdir1, subdir2, file])
        );
    }

    #[test]
    fn write_to_a_hardlink_pointed_to_the_file_in_the_watched_dir_doesnt_trigger_an_event() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let subdir = tmpdir.path().join("subdir");
        let subdir2 = tmpdir.path().join("subdir2");
        let file = subdir.join("file");
        let hardlink = subdir2.join("hardlink");

        std::fs::create_dir(&subdir).expect("create");
        std::fs::create_dir(&subdir2).expect("create");
        std::fs::write(&file, "").expect("file");
        std::fs::hard_link(&file, &hardlink).expect("hardlink");

        watcher.watch_nonrecursively(&subdir);

        std::fs::write(&hardlink, "123123").expect("write to the hard link");

        rx.wait_ordered_exact([expected(&subdir).access_open_any().optional()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), subdir])
        );
    }

    #[test]
    #[ignore = "see https://github.com/notify-rs/notify/issues/727"]
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

        rx.ensure_empty_with_wait();

        watcher.watch_recursively(&path);
        std::fs::File::create_new(&file).expect("create");

        rx.wait_ordered([
            expected(&file).create_file(),
            expected(&file).access_open_any(),
            expected(&file).access_close_write(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), path, deep])
        );
    }
}
