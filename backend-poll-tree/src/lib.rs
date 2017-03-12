extern crate notify_backend as backend;
extern crate futures;
extern crate walkdir;

use backend::prelude::*;
use backend::Buffer;

use futures::{Poll, Stream};
use std::path::PathBuf;

pub struct Backend {
    buffer: Buffer,
    trees: Vec<String>,
    watches: Vec<PathBuf>
}

impl NotifyBackend for Backend {
    fn capabilities() -> Vec<Capability> {
        vec![
            Capability::FollowSymlinks,
            Capability::WatchFiles,
            Capability::WatchFolders,
            Capability::WatchNewFolders,
            Capability::WatchRecursively,
        ]
    }

    fn new(paths: Vec<PathBuf>) -> BackendResult<Backend> {
        Ok(Backend { buffer: Buffer::new(), trees: vec![], watches: paths })
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

        // QUESTION: trigger resolves here? or on an interval in a thread?

        self.buffer.poll()
    }
}
