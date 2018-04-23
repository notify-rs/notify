//! Notify Backend crate for BSD (or others) kqueue.

#![deny(missing_docs)]

extern crate notify_backend as backend;
extern crate kqueue;
extern crate futures;

use backend::prelude::*;
use backend::Buffer;
use futures::{Poll, Stream};
use kqueue::{Event as KEvent, EventData, EventFilter, Ident, Vnode};
use std::path::PathBuf;

/// A Notify Backend for [kqueue].
///
/// Kqueue has been in *BSD since 2000.
///
/// This backend can only natively watch file, or more precisely, file descriptors.
///
/// The `poll()` method runs a loop 50 times, checking for events repeatedly. This is a limitation
/// of the underlying [kqueue binding].
///
/// Kqueue emits several special events:
///
///  - `ModifyKind::Other("link")` is emitted on `NOTE_LINK`, i.e. when the link count on the file
///  is changed.
///
///  - `RemoveKind::Other("revoke")` is emitted on `NOTE_REVOKE`, i.e. when access to the file is
///  [revoked], or when the underlying filesystem is unmounted.
///
///  - `ModifyKind::Other("extend")` is emitted on `NOTE_EXTEND`. This is a _special_ special
///  event, because it should really be represented by `ModifyKind::Data(DataChange::Size)` in the
///  case of files, and `CreateKind::Any` in the case of folders. However, differentiating between
///  these cases would mean an extra `stat` call per event. For now, that was deemed too costly.
///
/// [kqueue]: https://www.freebsd.org/cgi/man.cgi?kqueue(2)
/// [kqueue binding]: https://docs.worrbase.com/rust/kqueue/
/// [revoked]: https://www.freebsd.org/cgi/man.cgi?revoke(2)
#[derive(Debug)]
pub struct Backend {
    buffer: Buffer,
    kqueue: kqueue::Watcher,
}

impl NotifyBackend for Backend {
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

    fn capabilities() -> Vec<Capability> {
        vec![
            Capability::WatchFiles,
        ]
    }

    fn await(&mut self) -> EmptyStreamResult {
        match self.kqueue.iter().next() {
            // kqueue's iterator implementation only returns None if the watcher is not started.
            // However, the watcher is always started when initialising it.
            None => unreachable!(),
            Some(e) => self.process_event(e)
        }
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

            self.process_event(event)?;
        }

        self.buffer.poll()
    }
}

impl Backend {
    fn process_event(&mut self, event: KEvent) -> EmptyStreamResult {
        let filename = match event.ident {
            Ident::Filename(_, f) => f,
            _ => return Ok(())
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
            EventData::Error(e) => return Err(e.into()),
            _ => return Ok(())
        };

        self.buffer.push(Event {
            kind: kind,
            paths: vec![PathBuf::from(filename)],
            relid: None
        });

        Ok(())
    }
}
