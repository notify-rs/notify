//! Watcher implementation for the inotify Linux API
//!
//! The inotify API provides a mechanism for monitoring filesystem events.  Inotify can be used to
//! monitor individual files, or to monitor directories.  When a directory is monitored, inotify
//! will return events for the directory itself, and for files inside the directory.

extern crate inotify as inotify_sys;
extern crate libc;
extern crate walkdir;

use mio::{self, EventLoop};
use self::inotify_sys::wrapper::{self, INotify, Watch};
use self::walkdir::WalkDir;
use std::collections::HashMap;
use std::fs::metadata;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::Builder as ThreadBuilder;
use super::{Error, Event, op, Op, Result, Watcher, RecursiveMode};

mod flags;

const INOTIFY: mio::Token = mio::Token(0);

/// Watcher implementation based on inotify
pub struct INotifyWatcher(mio::Sender<EventLoopMsg>);

struct INotifyHandler {
    inotify: Option<INotify>,
    tx: Sender<Event>,
    watches: HashMap<PathBuf, (Watch, flags::Mask, bool)>,
    paths: HashMap<Watch, PathBuf>,
}

enum EventLoopMsg {
    AddWatch(PathBuf, RecursiveMode, Sender<Result<()>>),
    RemoveWatch(PathBuf, Sender<Result<()>>),
    Shutdown,
}

impl mio::Handler for INotifyHandler {
    type Timeout = ();
    type Message = EventLoopMsg;

    fn ready(&mut self,
             _event_loop: &mut EventLoop<INotifyHandler>,
             token: mio::Token,
             events: mio::EventSet) {
        match token {
            INOTIFY => {
                assert!(events.is_readable());

                let mut add_watches = Vec::new();
                let mut remove_watches = Vec::new();

                if let Some(ref mut inotify) = self.inotify {
                    match inotify.available_events() {
                        Ok(events) => {
                            assert!(!events.is_empty());

                            for e in events {
                                handle_event(e.clone(), &self.tx, &self.watches, &self.paths, &mut add_watches, &mut remove_watches);
                            }
                        }
                        Err(e) => {
                            let _ = self.tx.send(Event {
                                path: None,
                                op: Err(Error::Io(e)),
                            });
                        }
                    }
                }

                for path in remove_watches {
                    let _ = self.remove_watch(path, true);
                }

                for path in add_watches {
                    let _ = self.add_watch(path, true, false);
                }
            }
            _ => unreachable!(),
        }
    }

    fn notify(&mut self, event_loop: &mut EventLoop<INotifyHandler>, msg: EventLoopMsg) {
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
                event_loop.shutdown();
            }
        }
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

impl INotifyHandler {
    fn add_watch(&mut self, path: PathBuf, is_recursive: bool, mut watch_self: bool) -> Result<()> {
        let metadata = try!(metadata(&path).map_err(Error::Io));

        if !metadata.is_dir() || !is_recursive {
            return self.add_single_watch(path, false, true);
        }

        for entry in WalkDir::new(path)
                         .follow_links(true)
                         .into_iter()
                         .filter_map(|e| filter_dir(e)) {
            try!(self.add_single_watch(entry.path().to_path_buf(), is_recursive, watch_self));
            watch_self = false;
        }

        Ok(())
    }

    fn add_single_watch(&mut self, path: PathBuf, is_recursive: bool, watch_self: bool) -> Result<()> {
        let mut flags = flags::IN_ATTRIB | flags::IN_CREATE | flags::IN_DELETE |
                        flags::IN_MODIFY | flags::IN_MOVED_FROM | flags::IN_MOVED_TO;

        if watch_self {
            flags.insert(flags::IN_DELETE_SELF);
            flags.insert(flags::IN_MOVE_SELF);
        }

        if let Some(&(_, old_flags, _)) = self.watches.get(&path) {
            flags.insert(old_flags);
            flags.insert(flags::IN_MASK_ADD);
        }

        if let Some(ref inotify) = self.inotify {
            match inotify.add_watch(&path, flags.bits()) {
                Err(e) => Err(Error::Io(e)),
                Ok(w) => {
                    flags.remove(flags::IN_MASK_ADD);
                    self.watches.insert(path.clone(), (w, flags, is_recursive));
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
            None => return Err(Error::WatchNotFound),
            Some((w, _, is_recursive)) => {
                if let Some(ref inotify) = self.inotify {
                    try!(inotify.rm_watch(w).map_err(Error::Io));
                    self.paths.remove(&w);

                    if is_recursive || remove_recursive {
                        let mut remove_list = Vec::new();
                        for (w, p) in &self.paths {
                            if p.starts_with(&path) {
                                try!(inotify.rm_watch(*w).map_err(Error::Io));
                                self.watches.remove(p);
                                remove_list.push(*w);
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
        if let Some(ref inotify) = self.inotify {
            for w in self.paths.keys() {
                try!(inotify.rm_watch(*w).map_err(Error::Io));
            }
            self.watches.clear();
            self.paths.clear();
        }
        Ok(())
    }
}

#[inline]
fn handle_event(event: wrapper::Event,
                tx: &Sender<Event>,
                watches: &HashMap<PathBuf, (Watch, flags::Mask, bool)>,
                paths: &HashMap<Watch, PathBuf>,
                add_watches: &mut Vec<PathBuf>,
                remove_watches: &mut Vec<PathBuf>)
{
    let path = if event.name.is_empty() {
        match paths.get(&event.wd) {
            Some(p) => Some(p.clone()),
            None => None,
        }
    } else {
        paths.get(&event.wd).map(|root| root.join(&event.name))
    };

    let mut o = Op::empty();
    if event.is_create() || event.is_moved_to() {
        o.insert(op::CREATE);

        if let Some(ref path) = path {
            if event.is_dir() {
                if let Some(parent_path) = path.parent() {
                    if let Some(&(_, _, is_recursive)) = watches.get(parent_path) {
                        if is_recursive {
                            (*add_watches).push(path.to_owned());
                        }
                    }
                }
            }
        }
    }
    if event.is_delete_self() || event.is_delete() {
        o.insert(op::REMOVE);

        if let Some(ref path) = path {
            if event.is_dir() {
                (*remove_watches).push(path.to_owned());
            }
        }
    }
    if event.is_modify() {
        o.insert(op::WRITE);
    }
    if event.is_move_self() || event.is_moved_from() {
        o.insert(op::RENAME);

        if let Some(ref path) = path {
            if event.is_dir() {
                (*remove_watches).push(path.to_owned());
            }
        }
    }
    if event.is_attrib() {
        o.insert(op::CHMOD);
    }
    if event.is_ignored() {
        o.insert(op::IGNORED);
    }

    let _ = tx.send(Event {
        path: path,
        op: Ok(o),
    });
}

impl Watcher for INotifyWatcher {
    fn new(tx: Sender<Event>) -> Result<INotifyWatcher> {
        INotify::init()
            .and_then(|inotify| EventLoop::new().map(|l| (inotify, l)))
            .and_then(|(inotify, mut event_loop)| {
                let inotify_fd = inotify.fd;
                let evented_inotify = mio::unix::EventedFd(&inotify_fd);

                let handler = INotifyHandler {
                    inotify: Some(inotify),
                    tx: tx,
                    watches: HashMap::new(),
                    paths: HashMap::new(),
                };

                event_loop.register(&evented_inotify,
                                    INOTIFY,
                                    mio::EventSet::readable(),
                                    mio::PollOpt::level())
                          .map(|_| (event_loop, handler))
            })
            .map(|(mut event_loop, mut handler)| {
                let channel = event_loop.channel();

                ThreadBuilder::new()
                    .name("INotify Watcher".to_owned())
                    .spawn(move || event_loop.run(&mut handler))
                    .unwrap();

                INotifyWatcher(channel)
            })
            .map_err(Error::Io)
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let msg = EventLoopMsg::AddWatch(path.as_ref().to_owned(), recursive_mode, tx);

        // we expect the event loop to live and reply => unwraps must not panic
        self.0.send(msg).unwrap();
        rx.recv().unwrap()
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let msg = EventLoopMsg::RemoveWatch(path.as_ref().to_owned(), tx);

        // we expect the event loop to live and reply => unwraps must not panic
        self.0.send(msg).unwrap();
        rx.recv().unwrap()
    }
}

impl Drop for INotifyWatcher {
    fn drop(&mut self) {
        // we expect the event loop to live => unwrap must not panic
        self.0.send(EventLoopMsg::Shutdown).unwrap();
    }
}
