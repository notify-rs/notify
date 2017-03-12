extern crate notify_backend as backend;
extern crate kqueue;
extern crate futures;

use backend::prelude::*;
use backend::Buffer;
use futures::{Poll, Stream};
use std::path::PathBuf;

pub struct Backend {
    buffer: Buffer,
    kqueue: kqueue::Watcher,
}

impl NotifyBackend for Backend {
    fn capabilities() -> Vec<Capability> {
        vec![
            Capability::WatchFiles,
        ]
    }

    fn new(paths: Vec<PathBuf>) -> BackendResult<Backend> {
        let watcher = kqueue::Watcher::new()?;
        Ok(Backend { buffer: Buffer::new(), kqueue: watcher })
    }
}

impl Drop for Backend {
    fn drop(&mut self) {}
}

impl Stream for Backend {
    type Item = StreamItem;
    type Error = StreamError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.buffer.closed() {
            return self.buffer.poll()
        }

        self.buffer.poll()
    }
}
