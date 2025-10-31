//! Watcher implementation for the inotify Linux API
//!
//! The inotify API provides a mechanism for monitoring filesystem events.  Inotify can be used to
//! monitor individual files, or to monitor directories.  When a directory is monitored, inotify
//! will return events for the directory itself, and for files inside the directory.

use super::event::*;
use super::{Config, Error, ErrorKind, EventHandler, RecursiveMode, Result, Watcher};
use crate::{bounded, unbounded, BoundSender, Receiver, Sender};
use inotify as inotify_sys;
use inotify_sys::{EventMask, Inotify, WatchDescriptor, WatchMask};
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs::metadata;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use walkdir::WalkDir;

const INOTIFY: mio::Token = mio::Token(0);
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
    inotify: Option<Inotify>,
    event_handler: Box<dyn EventHandler>,
    /// PathBuf -> (WatchDescriptor, WatchMask, is_recursive, is_dir)
    watches: HashMap<PathBuf, (WatchDescriptor, WatchMask, bool, bool)>,
    paths: HashMap<WatchDescriptor, PathBuf>,
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
    AddWatch(PathBuf, RecursiveMode, Sender<Result<()>>),
    RemoveWatch(PathBuf, Sender<Result<()>>),
    Shutdown,
    Configure(Config, BoundSender<Result<bool>>),
}

#[inline]
fn add_watch_by_event(
    path: &PathBuf,
    event: &inotify_sys::Event<&OsStr>,
    watches: &HashMap<PathBuf, (WatchDescriptor, WatchMask, bool, bool)>,
    add_watches: &mut Vec<PathBuf>,
) {
    if event.mask.contains(EventMask::ISDIR) {
        if let Some(parent_path) = path.parent() {
            if let Some(&(_, _, is_recursive, _)) = watches.get(parent_path) {
                if is_recursive {
                    add_watches.push(path.to_owned());
                }
            }
        }
    }
}

#[inline]
fn remove_watch_by_event(
    path: &PathBuf,
    watches: &HashMap<PathBuf, (WatchDescriptor, WatchMask, bool, bool)>,
    remove_watches: &mut Vec<PathBuf>,
) {
    if watches.contains_key(path) {
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
            watches: HashMap::new(),
            paths: HashMap::new(),
            rename_event: None,
            follow_links,
        };
        Ok(event_loop)
    }

    // Run the event loop.
    pub fn run(self) {
        let _ = thread::Builder::new()
            .name("notify-rs inotify loop".to_string())
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
                Err(e) => panic!("poll failed: {}", e),
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
            INOTIFY => {
                // inotify has something to tell us.
                self.handle_inotify()
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
                EventLoopMsg::Shutdown => {
                    let _ = self.remove_all_watches();
                    if let Some(inotify) = self.inotify.take() {
                        let _ = inotify.close();
                    }
                    self.running = false;
                    break;
                }
                EventLoopMsg::Configure(config, tx) => {
                    self.configure_raw_mode(config, tx);
                }
            }
        }
    }

    fn configure_raw_mode(&mut self, _config: Config, tx: BoundSender<Result<bool>>) {
        tx.send(Ok(false))
            .expect("configuration channel disconnected");
    }

    fn handle_inotify(&mut self) {
        let mut add_watches = Vec::new();
        let mut remove_watches = Vec::new();

        if let Some(ref mut inotify) = self.inotify {
            let mut buffer = [0; 1024];
            // Read all buffers available.
            loop {
                match inotify.read_events(&mut buffer) {
                    Ok(events) => {
                        let mut num_events = 0;
                        for event in events {
                            log::trace!("inotify event: {event:?}");

                            num_events += 1;
                            if event.mask.contains(EventMask::Q_OVERFLOW) {
                                let ev = Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan));
                                self.event_handler.handle_event(ev);
                            }

                            let path = match event.name {
                                Some(name) => self.paths.get(&event.wd).map(|root| root.join(name)),
                                None => self.paths.get(&event.wd).cloned(),
                            };

                            let path = match path {
                                Some(path) => path,
                                None => {
                                    log::debug!("inotify event with unknown descriptor: {event:?}");
                                    continue;
                                }
                            };

                            let mut evs = Vec::new();

                            if event.mask.contains(EventMask::MOVED_FROM) {
                                remove_watch_by_event(&path, &self.watches, &mut remove_watches);

                                let event = Event::new(EventKind::Modify(ModifyKind::Name(
                                    RenameMode::From,
                                )))
                                .add_path(path.clone())
                                .set_tracker(event.cookie as usize);

                                self.rename_event = Some(event.clone());

                                evs.push(event);
                            } else if event.mask.contains(EventMask::MOVED_TO) {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To)))
                                        .set_tracker(event.cookie as usize)
                                        .add_path(path.clone()),
                                );

                                let trackers_match =
                                    self.rename_event.as_ref().and_then(|e| e.tracker())
                                        == Some(event.cookie as usize);

                                if trackers_match {
                                    let rename_event = self.rename_event.take().unwrap(); // unwrap is safe because `rename_event` must be set at this point
                                    evs.push(
                                        Event::new(EventKind::Modify(ModifyKind::Name(
                                            RenameMode::Both,
                                        )))
                                        .set_tracker(event.cookie as usize)
                                        .add_some_path(rename_event.paths.first().cloned())
                                        .add_path(path.clone()),
                                    );
                                }
                                add_watch_by_event(&path, &event, &self.watches, &mut add_watches);
                            }
                            if event.mask.contains(EventMask::MOVE_SELF) {
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
                            if event.mask.contains(EventMask::CREATE) {
                                evs.push(
                                    Event::new(EventKind::Create(
                                        if event.mask.contains(EventMask::ISDIR) {
                                            CreateKind::Folder
                                        } else {
                                            CreateKind::File
                                        },
                                    ))
                                    .add_path(path.clone()),
                                );
                                add_watch_by_event(&path, &event, &self.watches, &mut add_watches);
                            }
                            if event.mask.contains(EventMask::DELETE) {
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
                                remove_watch_by_event(&path, &self.watches, &mut remove_watches);
                            }
                            if event.mask.contains(EventMask::DELETE_SELF) {
                                let remove_kind = match self.watches.get(&path) {
                                    Some(&(_, _, _, true)) => RemoveKind::Folder,
                                    Some(&(_, _, _, false)) => RemoveKind::File,
                                    None => RemoveKind::Other,
                                };
                                evs.push(
                                    Event::new(EventKind::Remove(remove_kind))
                                        .add_path(path.clone()),
                                );
                                remove_watch_by_event(&path, &self.watches, &mut remove_watches);
                            }
                            if event.mask.contains(EventMask::MODIFY) {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Data(
                                        DataChange::Any,
                                    )))
                                    .add_path(path.clone()),
                                );
                            }
                            if event.mask.contains(EventMask::CLOSE_WRITE) {
                                evs.push(
                                    Event::new(EventKind::Access(AccessKind::Close(
                                        AccessMode::Write,
                                    )))
                                    .add_path(path.clone()),
                                );
                            }
                            if event.mask.contains(EventMask::CLOSE_NOWRITE) {
                                evs.push(
                                    Event::new(EventKind::Access(AccessKind::Close(
                                        AccessMode::Read,
                                    )))
                                    .add_path(path.clone()),
                                );
                            }
                            if event.mask.contains(EventMask::ATTRIB) {
                                evs.push(
                                    Event::new(EventKind::Modify(ModifyKind::Metadata(
                                        MetadataKind::Any,
                                    )))
                                    .add_path(path.clone()),
                                );
                            }
                            if event.mask.contains(EventMask::OPEN) {
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

        for path in remove_watches {
            self.remove_watch(path, true).ok();
        }

        for path in add_watches {
            if let Err(add_watch_error) = self.add_watch(path, true, false) {
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
    }

    fn add_watch(&mut self, path: PathBuf, is_recursive: bool, mut watch_self: bool) -> Result<()> {
        // If the watch is not recursive, or if we determine (by stat'ing the path to get its
        // metadata) that the watched path is not a directory, add a single path watch.
        if !is_recursive || !metadata(&path).map_err(Error::io_watch)?.is_dir() {
            return self.add_single_watch(path, false, true);
        }

        for entry in WalkDir::new(path)
            .follow_links(self.follow_links)
            .into_iter()
            .filter_map(filter_dir)
        {
            self.add_single_watch(entry.into_path(), is_recursive, watch_self)?;
            watch_self = false;
        }

        Ok(())
    }

    fn add_single_watch(
        &mut self,
        path: PathBuf,
        is_recursive: bool,
        watch_self: bool,
    ) -> Result<()> {
        let mut watchmask = WatchMask::ATTRIB
            | WatchMask::CREATE
            | WatchMask::OPEN
            | WatchMask::DELETE
            | WatchMask::CLOSE_WRITE
            | WatchMask::MODIFY
            | WatchMask::MOVED_FROM
            | WatchMask::MOVED_TO;

        if watch_self {
            watchmask.insert(WatchMask::DELETE_SELF);
            watchmask.insert(WatchMask::MOVE_SELF);
        }

        if let Some(&(_, old_watchmask, _, _)) = self.watches.get(&path) {
            watchmask.insert(old_watchmask);
            watchmask.insert(WatchMask::MASK_ADD);
        }

        if let Some(ref mut inotify) = self.inotify {
            log::trace!("adding inotify watch: {}", path.display());

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
                    watchmask.remove(WatchMask::MASK_ADD);
                    let is_dir = metadata(&path).map_err(Error::io)?.is_dir();
                    self.watches
                        .insert(path.clone(), (w.clone(), watchmask, is_recursive, is_dir));
                    self.paths.insert(w, path);
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

    fn remove_watch(&mut self, path: PathBuf, remove_recursive: bool) -> Result<()> {
        match self.watches.remove(&path) {
            None => return Err(Error::watch_not_found().add_path(path)),
            Some((w, _, is_recursive, _)) => {
                if let Some(ref mut inotify) = self.inotify {
                    let mut inotify_watches = inotify.watches();
                    log::trace!("removing inotify watch: {}", path.display());

                    inotify_watches
                        .remove(w.clone())
                        .map_err(|e| Error::io(e).add_path(path.clone()))?;
                    self.paths.remove(&w);

                    if is_recursive || remove_recursive {
                        let mut remove_list = Vec::new();
                        for (w, p) in &self.paths {
                            if p.starts_with(&path) {
                                inotify_watches
                                    .remove(w.clone())
                                    .map_err(|e| Error::io(e).add_path(p.into()))?;
                                self.watches.remove(p);
                                remove_list.push(w.clone());
                            }
                        }
                        for w in remove_list {
                            self.paths.remove(&w);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn remove_all_watches(&mut self) -> Result<()> {
        if let Some(ref mut inotify) = self.inotify {
            let mut inotify_watches = inotify.watches();
            for (w, p) in &self.paths {
                inotify_watches
                    .remove(w.clone())
                    .map_err(|e| Error::io(e).add_path(p.into()))?;
            }
            self.watches.clear();
            self.paths.clear();
        }
        Ok(())
    }
}

/// return `DirEntry` when it is a directory
fn filter_dir(e: walkdir::Result<walkdir::DirEntry>) -> Option<walkdir::DirEntry> {
    if let Ok(e) = e {
        if let Ok(metadata) = e.metadata() {
            if metadata.is_dir() {
                return Some(e);
            }
        }
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
        let waker = event_loop.event_loop_waker.clone();
        event_loop.run();
        Ok(INotifyWatcher { channel, waker })
    }

    fn watch_inner(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        let pb = if path.is_absolute() {
            path.to_owned()
        } else {
            let p = env::current_dir().map_err(Error::io)?;
            p.join(path)
        };
        let (tx, rx) = unbounded();
        let msg = EventLoopMsg::AddWatch(pb, recursive_mode, tx);

        // we expect the event loop to live and reply => unwraps must not panic
        self.channel.send(msg).unwrap();
        self.waker.wake().unwrap();
        rx.recv().unwrap()
    }

    fn unwatch_inner(&mut self, path: &Path) -> Result<()> {
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
    fn new<F: EventHandler>(event_handler: F, config: Config) -> Result<Self> {
        Self::from_event_handler(Box::new(event_handler), config.follow_symlinks())
    }

    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        self.watch_inner(path, recursive_mode)
    }

    fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.unwatch_inner(path)
    }

    fn configure(&mut self, config: Config) -> Result<bool> {
        let (tx, rx) = bounded(1);
        self.channel.send(EventLoopMsg::Configure(config, tx))?;
        self.waker.wake()?;
        rx.recv()?
    }

    fn kind() -> crate::WatcherKind {
        crate::WatcherKind::Inotify
    }
}

impl Drop for INotifyWatcher {
    fn drop(&mut self) {
        // we expect the event loop to live => unwrap must not panic
        self.channel.send(EventLoopMsg::Shutdown).unwrap();
        self.waker.wake().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{atomic::AtomicBool, mpsc},
        thread::available_parallelism,
        time::Duration,
    };

    use super::*;
    use crate::test::*;
    use pretty_assertions::assert_eq;

    fn not_flaky(event: &Event) -> bool {
        // Access events aren't real "flaky", but they are to noisy
        if event.kind.is_access() {
            return false;
        }

        // That kind of an event requires the both (RenameMode::From and RenameMode::To) events
        // are read in the same batch, but this is not guaranteed.
        if matches!(
            event.kind,
            EventKind::Modify(ModifyKind::Name(RenameMode::Both))
        ) {
            return false;
        }

        true
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
            RecursiveMode::NonRecursive,
        );

        assert!(matches!(
            result,
            Err(Error {
                paths: _,
                kind: ErrorKind::PathNotFound
            })
        ))
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
            .watch(tmpdir.path(), RecursiveMode::Recursive)
            .unwrap();
        watcher
            .watch(proc_path, RecursiveMode::NonRecursive)
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
                let stop = stop.clone();
                thread::spawn(move || {
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = std::fs::File::open(&file_path).unwrap();
                    }
                })
            })
            .collect();

        let non_recursive = RecursiveMode::NonRecursive;
        for _ in 0..(handles.len() * 4) {
            inotify.watch(dir_path, non_recursive).unwrap();
            inotify.unwatch(dir_path).unwrap();
        }

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        handles
            .into_iter()
            .for_each(|handle| handle.join().ok().unwrap_or_default());

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
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        rx.wait([event(EventKind::Create(CreateKind::File), &path)]);
    }

    #[test]
    fn create_dir() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create");

        rx.wait([event(EventKind::Create(CreateKind::Folder), &path)]);
    }

    #[test]
    fn write_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        std::fs::write(&path, b"123").expect("write");

        rx.wait([event(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            &path,
        )]);
    }

    #[test]
    fn chmod_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();

        let path = tmpdir.path().join("entry");
        let file = std::fs::File::create_new(&path).expect("create");
        let mut permissions = file.metadata().expect("metadata").permissions();
        permissions.set_readonly(true);

        watcher.watch_recursively(&tmpdir);
        file.set_permissions(permissions).expect("set_permissions");

        rx.wait([event(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
            &path,
        )]);
    }

    #[test]
    fn rename_file() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        let new_path = tmpdir.path().join("renamed");

        std::fs::rename(&path, &new_path).expect("rename");

        let events = [rx.recv(), rx.recv()]
            .into_iter()
            .map(|e| (e.attrs.tracker(), ExpectedEvent::from_event(e)))
            .collect::<Vec<_>>();

        let cookie = events.first().and_then(|e| e.0);
        assert!(cookie.is_some(), "cookie is none: [events:#?]");

        assert_eq!(
            &events,
            &[
                (
                    cookie,
                    event(EventKind::Modify(ModifyKind::Name(RenameMode::From)), path)
                ),
                (
                    cookie,
                    event(
                        EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                        new_path
                    )
                ),
            ]
        );
    }

    #[test]
    fn move_out_of_watched_dir() {
        let tmpdir = testdir();
        let subdir = tmpdir.path().join("subdir");
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();

        let path = subdir.join("entry");
        std::fs::create_dir_all(&subdir).expect("create_dir_all");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&subdir);
        let new_path = tmpdir.path().join("entry");

        std::fs::rename(&path, &new_path).expect("rename");

        let received = rx.recv();
        assert!(
            received.attrs.tracker().is_some(),
            "cookie is none: [event:#?]"
        );

        assert_eq!(
            ExpectedEvent::from_event(received),
            event(EventKind::Modify(ModifyKind::Name(RenameMode::From)), path)
        );
        rx.ensure_empty();
    }

    #[test]
    fn create_write_write_rename_write_remove() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();

        let path = tmpdir.path().join("entry");
        let new_path = tmpdir.path().join("renamed");

        watcher.watch_recursively(&tmpdir);
        std::fs::File::create_new(&path).expect("create");
        std::fs::write(&path, b"123").expect("write 1");
        std::fs::write(&path, b"321321312eewfrserf").expect("write 2");
        std::fs::rename(&path, &new_path).expect("rename");
        std::fs::write(&new_path, b"1").expect("write 3");
        std::fs::remove_file(&new_path).expect("remove");

        let mut events = Vec::new();
        for event in rx.iter().filter(not_flaky) {
            let last_event = event.kind.is_remove();
            events.push(ExpectedEvent::from_event(event));
            if last_event {
                break;
            }
        }

        assert_eq!(
            &events,
            &[
                event(EventKind::Create(CreateKind::File), &path),
                event(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path),
                event(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path),
                event(EventKind::Modify(ModifyKind::Name(RenameMode::From)), &path),
                event(
                    EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                    &new_path
                ),
                event(
                    EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                    &new_path
                ),
                event(EventKind::Remove(RemoveKind::File), &new_path),
            ]
        )
    }

    #[test]
    fn rename_twice() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        let new_path1 = tmpdir.path().join("renamed1");
        let new_path2 = tmpdir.path().join("renamed2");

        std::fs::rename(&path, &new_path1).expect("rename1");
        std::fs::rename(&new_path1, &new_path2).expect("rename2");

        let mut events = Vec::new();
        let mut cookies = Vec::new();
        for event in rx.iter().filter(not_flaky) {
            let Some(tracker) = event.attrs.tracker() else {
                continue;
            };

            if cookies.last() != Some(&tracker) {
                cookies.push(tracker);
            }

            let last_event =
                event.kind.is_modify() && event.paths.first().is_some_and(|p| p == &new_path2);

            events.push((tracker, ExpectedEvent::from_event(event)));
            if last_event {
                break;
            }
        }

        assert_eq!(cookies.len(), 2);

        assert_eq!(
            &events,
            &[
                (
                    cookies[0],
                    event(EventKind::Modify(ModifyKind::Name(RenameMode::From)), &path)
                ),
                (
                    cookies[0],
                    event(
                        EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                        &new_path1
                    )
                ),
                (
                    cookies[1],
                    event(
                        EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                        &new_path1
                    )
                ),
                (
                    cookies[1],
                    event(
                        EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                        &new_path2
                    )
                ),
            ]
        );
    }

    #[test]
    fn set_file_mtime() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();

        let path = tmpdir.path().join("entry");
        let file = std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);

        file.set_modified(
            std::time::SystemTime::now()
                .checked_sub(Duration::from_secs(60 * 60))
                .expect("time"),
        )
        .expect("set_time");

        assert_eq!(
            ExpectedEvent::from_event(rx.recv()),
            event(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path)
        );
        rx.ensure_empty();
    }

    #[test]
    fn write_file_non_recursive_watch() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = channel::<INotifyWatcher>();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_nonrecursively(&path);

        std::fs::write(&path, b"123").expect("write");

        let actual = rx
            .iter()
            .find(|e| e.kind.is_modify())
            .map(ExpectedEvent::from_event);

        assert_eq!(
            actual,
            Some(event(
                EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                &path
            ))
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

        let (mut watcher, mut rx) = channel::<INotifyWatcher>();

        watcher.watch_recursively(&tmpdir);

        std::fs::create_dir_all(&nested9).expect("create_dir_all");

        let mut events = Vec::new();
        for event in rx.iter().filter(|e| !e.kind.is_access()) {
            let last_event =
                event.kind.is_create() && event.paths.first().is_some_and(|p| p == &nested9);
            events.push(ExpectedEvent::from_event(event));
            if last_event {
                break;
            }
        }

        assert_eq!(
            &events,
            &[
                event(EventKind::Create(CreateKind::Folder), &nested1),
                event(EventKind::Create(CreateKind::Folder), &nested2),
                event(EventKind::Create(CreateKind::Folder), &nested3),
                event(EventKind::Create(CreateKind::Folder), &nested4),
                event(EventKind::Create(CreateKind::Folder), &nested5),
                event(EventKind::Create(CreateKind::Folder), &nested6),
                event(EventKind::Create(CreateKind::Folder), &nested7),
                event(EventKind::Create(CreateKind::Folder), &nested8),
                event(EventKind::Create(CreateKind::Folder), &nested9),
            ]
        )
    }
}
