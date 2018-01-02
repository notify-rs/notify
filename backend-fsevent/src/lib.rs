//! Notify Backend using the Darwin FSEvents API.

// For more information on the FSEvents API, the best resource is in the system
// headers; located on your Macintosh at
// /System/Library/Frameworks/CoreServices.framework/Frameworks/FSEvents.framework/Headers/FSEvents.h

extern crate notify_backend as backend;
extern crate libc;
extern crate futures;
extern crate fsevent as fsevent_rs;
extern crate fsevent_sys;

mod watcher;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, Condvar};
use std::collections::VecDeque;

use futures::{Async, Poll, Stream};

use backend::prelude::*;
use watcher::FsEventWatcher;

pub type WaitQueue = Arc<(Mutex<VecDeque<Event>>, Condvar)>;

pub struct Backend {
    watcher: FsEventWatcher,
    queue: WaitQueue,
}

impl NotifyBackend for Backend {
    fn new(paths: Vec<PathBuf>) -> BackendResult<BoxedBackend> {
        Ok(Box::new(Backend::new(paths)))
    }

    fn caps(&self) -> Vec<Capability> {
        Self::capabilities()
    }

    fn capabilities() -> Vec<Capability> {
        vec![
            Capability::FollowSymlinks,
            Capability::WatchEntireFilesystem,
            Capability::WatchFiles,
            Capability::WatchFolders,
            Capability::WatchNewFolders,
            Capability::WatchRecursively,
        ]
    }

    fn await(&mut self) -> EmptyStreamResult {
        let &(ref deque, ref cond) = &*self.queue;
        let guard = deque.lock().unwrap();
		let result = cond.wait(guard);
        Ok(())
    }
}

impl Backend {
    fn new(paths: Vec<PathBuf>) -> Self {
        let queue = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        Backend {
            watcher: FsEventWatcher::new(paths, queue.clone()),
            queue: queue,
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // cleanup occurs in FsEventWatcher::drop
    }
}

impl Stream for Backend {
    type Item = StreamItem;
    type Error = StreamError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let &(ref deque, _) = &*self.queue;
        let mut queue = deque.lock().unwrap();
        match queue.pop_front() {
            Some(e) => Ok(Async::Ready(Some(e))),
            None => Ok(Async::NotReady),
        }
    }
}


