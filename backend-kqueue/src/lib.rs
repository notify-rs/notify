extern crate notify_backend as backend;
extern crate kqueue;
extern crate futures;

use backend::prelude::*;
use backend::Buffer;
use futures::{Poll, Stream};
use kqueue::{EventData, EventFilter, Ident, Vnode};
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
        let mut watcher = kqueue::Watcher::new()?;

        for path in paths {
            watcher.add_filename(path, EventFilter::EVFILT_VNODE,
                kqueue::NOTE_ATTRIB | kqueue::NOTE_DELETE | kqueue::NOTE_RENAME
                | kqueue::NOTE_WRITE | kqueue::NOTE_REVOKE | kqueue::NOTE_EXTEND
            )?;
        }

        watcher.watch()?;

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
        for _ in 0..50 {
            let event = match self.kqueue.poll(None) {
                None => continue,
                Some(e) => e
            };

            let filename = match event.ident {
                Ident::Filename(_, f) => f,
                _ => continue
            };

            let kind = match event.data {
                EventData::Vnode(v) => match v {
                    Vnode::Delete => EventKind::Remove(RemoveKind::Any),
                    Vnode::Write => EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                    Vnode::Extend => EventKind::Modify(ModifyKind::Other("extend".into())),
                    Vnode::Attrib => EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
                    Vnode::Link => EventKind::Modify(ModifyKind::Other("link".into())),
                    Vnode::Rename => EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                    Vnode::Revoke => EventKind::Remove(RemoveKind::Other("revoke".into())),
                    _ => EventKind::Any
                },
                _ => continue
            };

            self.buffer.push(Event {
                kind: kind,
                paths: vec![PathBuf::from(filename)],
                relid: None
            });
        }

        self.buffer.poll()
    }
}
