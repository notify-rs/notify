extern crate notify_backend as backend;
extern crate walkdir;

use backend::prelude::*;
use backend::Buffer;

pub struct Backend {
    buffer: Buffer,
    reg: MioRegistration,
    trees: Vec<String>,
    watches: Vec<PathBuf>,
}

impl NotifyBackend for Backend {
    fn new(_paths: Vec<PathBuf>) -> NewBackendResult {
        Err(BackendError::NotImplemented)
    }

    fn caps(&self) -> Vec<Capability> {
        Self::capabilities()
    }

    fn capabilities() -> Vec<Capability> {
        vec![
            Capability::FollowSymlinks,
            Capability::WatchFiles,
            Capability::WatchFolders,
            Capability::WatchNewFolders,
            Capability::WatchRecursively,
        ]
    }
}

impl Drop for Backend {
    fn drop(&mut self) {}
}

impl Evented for Backend {
    fn register(&self, poll: &MioPoll, token: MioToken, interest: MioReady, opts: MioPollOpt) -> MioResult {
        self.reg.register(poll, token, interest, opts)
    }

    fn reregister(&self, poll: &MioPoll, token: MioToken, interest: MioReady, opts: MioPollOpt) -> MioResult {
        self.reg.reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &MioPoll) -> MioResult {
        self.reg.deregister(poll)
    }
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
