//! Watcher implementation for the kqueue API
//!
//! The kqueue() system call provides a generic method of notifying the user
//! when an event happens or a condition holds, based on the results of small
//! pieces of kernel code termed filters.

use super::event::*;
use super::{Error, EventFn, RecursiveMode, Result, Watcher};
use crossbeam_channel::{unbounded, Sender};
use kqueue::{EventData, EventFilter, FilterFlag, Ident};
use std::collections::HashMap;
use std::env;
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
    event_loop_tx: crossbeam_channel::Sender<EventLoopMsg>,
    event_loop_rx: crossbeam_channel::Receiver<EventLoopMsg>,
    kqueue: kqueue::Watcher,
    event_fn: Box<dyn EventFn>,
    watches: HashMap<PathBuf, bool>,
}

/// Watcher implementation based on inotify
pub struct KqueueWatcher {
    channel: crossbeam_channel::Sender<EventLoopMsg>,
    waker: Arc<mio::Waker>,
}

enum EventLoopMsg {
    AddWatch(PathBuf, RecursiveMode, Sender<Result<()>>),
    RemoveWatch(PathBuf, Sender<Result<()>>),
    Shutdown,
}

impl EventLoop {
    pub fn new(kqueue: kqueue::Watcher, event_fn: Box<dyn EventFn>) -> Result<Self> {
        let (event_loop_tx, event_loop_rx) = crossbeam_channel::unbounded::<EventLoopMsg>();
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
            event_fn,
            watches: HashMap::new(),
        };
        Ok(event_loop)
    }

    // Run the event loop.
    pub fn run(self) {
        thread::spawn(|| self.event_loop_thread());
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
                self.handle_event(&event);
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
                    let _ = tx.send(self.add_watch(path, recursive_mode.is_recursive()));
                }
                EventLoopMsg::RemoveWatch(path, tx) => {
                    let _ = tx.send(self.remove_watch(path, false));
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

        println!("hello");
        loop {
            match self.kqueue.poll(None) {
                Some(event) => {
                    dbg!(&event);
                    match event {
                        kqueue::Event {
                            data: EventData::Vnode(data),
                            ident: Ident::Filename(_, path),
                        } => {
                            let path = PathBuf::from(path);
                            let event = match data {
                                /*
                                TODO: Differenciate folders and files
                                kqueue dosen't tell us if this was a file or a dir, so we
                                could only emulate this inotify behavior if we keep track of
                                all files and directories internally and then perform a
                                lookup.
                                */
                                kqueue::Vnode::Delete => {
                                    remove_watches.push(path.clone());
                                    Event::new(EventKind::Remove(RemoveKind::Any))
                                }

                                //data was write to this file
                                kqueue::Vnode::Write => Event::new(EventKind::Access(
                                    AccessKind::Close(AccessMode::Write),
                                )),

                                /*
                                Extend and Truncate are just different names for the same
                                operation, truncate is only used on FreeBSD, extend everwhere
                                else
                                */
                                kqueue::Vnode::Extend => Event::new(EventKind::Modify(
                                    ModifyKind::Data(DataChange::Size),
                                )),
                                kqueue::Vnode::Truncate => Event::new(EventKind::Modify(
                                    ModifyKind::Data(DataChange::Size),
                                )),

                                /*
                                this kevent has the same problem as the delete kevent. The
                                only way i can think of providing "better" event with more
                                information is to do the diff our self, while this maybe do
                                able of delete. In this case it would somewhat expensive to
                                keep track and compare ever peace of metadata for every file
                                */
                                kqueue::Vnode::Attrib => Event::new(EventKind::Modify(
                                    ModifyKind::Metadata(MetadataKind::Any),
                                )),

                                /*
                                The link count on a file changed => subdirectory created or
                                delete. Currently now idea how to track this.
                                */
                                //TODO: Find new files and track them
                                kqueue::Vnode::Link => {
                                    Event::new(EventKind::Modify(ModifyKind::Any))
                                }

                                //TODO: is the anyway to track this with kqueue
                                kqueue::Vnode::Rename => {
                                    remove_watches.push(path.clone());
                                    Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
                                }

                                // Access to the file was revoked via revoke(2) or the underlying file system was unmounted.
                                kqueue::Vnode::Revoke => {
                                    remove_watches.push(path.clone());
                                    Event::new(EventKind::Remove(RemoveKind::Any))
                                }
                            }
                            .add_path(path);
                            (self.event_fn)(Ok(event));
                        }
                        // as we don't add any other EVFILTER to kqueue we should never get here
                        kqueue::Event { ident: _, data: _ } => unreachable!(),
                    }
                    ()
                }
                None => break,
            }
        }

        for path in remove_watches {
            self.remove_watch(path, true).ok();
        }

        for path in add_watches {
            self.add_watch(path, true).ok();
        }
    }

    fn add_watch(&mut self, path: PathBuf, is_recursive: bool) -> Result<()> {
        // If the watch is not recursive, or if we determine (by stat'ing the path to get its
        // metadata) that the watched path is not a directory, add a single path watch.
        if !is_recursive || !metadata(&path).map_err(Error::io)?.is_dir() {
            return self.add_single_watch(path, false);
        }

        for entry in WalkDir::new(path)
            .follow_links(true)
            .into_iter()
            .filter_map(filter_dir)
        {
            self.add_single_watch(entry.path().to_path_buf(), is_recursive)?;
        }
        self.kqueue.watch()?;

        Ok(())
    }

    fn add_single_watch(&mut self, path: PathBuf, is_recursive: bool) -> Result<()> {
        let event_filter = EventFilter::EVFILT_VNODE;
        let filter_flags = FilterFlag::NOTE_DELETE
            | FilterFlag::NOTE_WRITE
            | FilterFlag::NOTE_EXTEND
            | FilterFlag::NOTE_ATTRIB
            | FilterFlag::NOTE_LINK
            | FilterFlag::NOTE_RENAME
            | FilterFlag::NOTE_REVOKE;

        self.kqueue
            .add_filename(&path, event_filter, filter_flags)?;
        self.watches.insert(path, is_recursive);
        self.kqueue.watch()?;
        Ok(())
    }

    fn remove_watch(&mut self, path: PathBuf, remove_recursive: bool) -> Result<()> {
        match self.watches.remove(&path) {
            None => return Err(Error::watch_not_found()),
            Some(is_recursive) => {
                self.kqueue
                    .remove_filename(&path, EventFilter::EVFILT_VNODE)
                    .map_err(|e| Error::io(e))?;

                if is_recursive || remove_recursive {
                    for entry in WalkDir::new(path)
                        .follow_links(true)
                        .into_iter()
                        .filter_map(filter_dir)
                    {
                        self.kqueue.remove_filename(
                            entry.path().to_path_buf(),
                            EventFilter::EVFILT_VNODE,
                        )?;
                    }
                }
                self.kqueue.watch()?;
            }
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

impl KqueueWatcher {
    fn from_event_fn(event_fn: Box<dyn EventFn>) -> Result<Self> {
        let kqueue = kqueue::Watcher::new()?;
        let event_loop = EventLoop::new(kqueue, event_fn)?;
        let channel = event_loop.event_loop_tx.clone();
        let waker = event_loop.event_loop_waker.clone();
        event_loop.run();
        Ok(KqueueWatcher { channel, waker })
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

impl Watcher for KqueueWatcher {
    fn new_immediate<F: EventFn>(event_fn: F) -> Result<KqueueWatcher> {
        KqueueWatcher::from_event_fn(Box::new(event_fn))
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        self.watch_inner(path.as_ref(), recursive_mode)
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        self.unwatch_inner(path.as_ref())
    }
}

impl Drop for KqueueWatcher {
    fn drop(&mut self) {
        // we expect the event loop to live => unwrap must not panic
        self.channel.send(EventLoopMsg::Shutdown).unwrap();
        self.waker.wake().unwrap();
    }
}
