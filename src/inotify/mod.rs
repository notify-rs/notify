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
use super::{Error, Event, op, Op, Result, Watcher};

mod flags;

const INOTIFY: mio::Token = mio::Token(0);

/// Watcher implementation based on inotify
pub struct INotifyWatcher(mio::Sender<EventLoopMsg>);

struct INotifyHandler {
    inotify: Option<INotify>,
    tx: Sender<Event>,
    watches: HashMap<PathBuf, (Watch, flags::Mask)>,
    paths: HashMap<Watch, PathBuf>,
}

enum EventLoopMsg {
    AddWatch(PathBuf, Sender<Result<()>>),
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

                if let Some(ref mut inotify) = self.inotify {
                    match inotify.available_events() {
                        Ok(events) => {
                            assert!(!events.is_empty());

                            for e in events {
                                handle_event(e.clone(), &self.tx, &self.paths)
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
            }
            _ => unreachable!(),
        }
    }

    fn notify(&mut self, event_loop: &mut EventLoop<INotifyHandler>, msg: EventLoopMsg) {
        match msg {
            EventLoopMsg::AddWatch(path, tx) => {
                let _ = tx.send(self.add_watch_recursively(path));
            }
            EventLoopMsg::RemoveWatch(path, tx) => {
                let _ = tx.send(self.remove_watch(path));
            }
            EventLoopMsg::Shutdown => {
                for path in self.watches.clone().keys() {
                    let _ = self.remove_watch(path.to_owned());
                }
                if let Some(inotify) = self.inotify.take() {
                    let _ = inotify.close();
                }

                event_loop.shutdown();
            }
        }
    }
}

/// return DirEntry when it is a directory
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
    fn add_watch_recursively(&mut self, path: PathBuf) -> Result<()> {
        match metadata(&path) {
            Err(e) => return Err(Error::Io(e)),
            Ok(m) => {
                if !m.is_dir() {
                    return self.add_watch(path);
                }
            }
        }

        for entry in WalkDir::new(path)
                         .follow_links(true)
                         .into_iter()
                         .filter_map(|e| filter_dir(e)) {
            try!(self.add_watch(entry.path().to_path_buf()));
        }

        Ok(())
    }

    fn add_watch(&mut self, path: PathBuf) -> Result<()> {
        let mut watching = flags::IN_ATTRIB | flags::IN_CREATE | flags::IN_DELETE |
                           flags::IN_DELETE_SELF | flags::IN_MODIFY |
                           flags::IN_MOVED_FROM |
                           flags::IN_MOVED_TO | flags::IN_MOVE_SELF;
        if let Some(p) = self.watches.get(&path) {
            watching.insert(p.1);
            watching.insert(flags::IN_MASK_ADD);
        }

        if let Some(ref inotify) = self.inotify {
            match inotify.add_watch(&path, watching.bits()) {
                Err(e) => Err(Error::Io(e)),
                Ok(w) => {
                    watching.remove(flags::IN_MASK_ADD);
                    self.watches.insert(path.clone(), (w, watching));
                    self.paths.insert(w, path);
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

    fn remove_watch(&mut self, path: PathBuf) -> Result<()> {
        match self.watches.remove(&path) {
            None => Err(Error::WatchNotFound),
            Some(p) => {
                let w = p.0;
                if let Some(ref inotify) = self.inotify {
                    match inotify.rm_watch(w) {
                        Err(e) => Err(Error::Io(e)),
                        Ok(_) => {
                            // Nothing depends on the value being gone
                            // from here now that inotify isn't watching.
                            self.paths.remove(&w);
                            Ok(())
                        }
                    }
                } else {
                    Ok(())
                }
            }
        }
    }
}

#[inline]
fn handle_event(event: wrapper::Event,
                tx: &Sender<Event>,
                paths: &HashMap<Watch, PathBuf>) {
    let mut o = Op::empty();
    if event.is_create() || event.is_moved_to() {
        o.insert(op::CREATE);
    }
    if event.is_delete_self() || event.is_delete() {
        o.insert(op::REMOVE);
    }
    if event.is_modify() {
        o.insert(op::WRITE);
    }
    if event.is_move_self() || event.is_moved_from() {
        o.insert(op::RENAME);
    }
    if event.is_attrib() {
        o.insert(op::CHMOD);
    }
    if event.is_ignored() {
        o.insert(op::IGNORED);
    }

    let path = if event.name.is_empty() {
        match paths.get(&event.wd) {
            Some(p) => Some(p.clone()),
            None => None,
        }
    } else {
        paths.get(&event.wd).map(|root| root.join(&event.name))
    };

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

    fn watch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let msg = EventLoopMsg::AddWatch(path.as_ref().to_owned(), tx);

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
