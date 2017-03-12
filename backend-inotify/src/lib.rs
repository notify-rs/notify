extern crate notify_backend as backend;
extern crate futures;
extern crate inotify;

use backend::prelude::*;
use backend::Buffer;

use futures::{Poll, Stream};
use inotify::{ffi, INotify};
use std::path::PathBuf;

pub struct Backend {
    inotify: INotify,
    buffer: Buffer
}

impl NotifyBackend for Backend {
    fn capabilities() -> Vec<Capability> {
        vec![
            Capability::EmitOnAccess,
            Capability::TrackRelated,
            Capability::WatchFiles,
            Capability::WatchFolders,
        ]
    }

    fn new(paths: Vec<PathBuf>) -> BackendResult<Backend> {
        let ino = INotify::init()
            .or_else(|err| Err(BackendError::Io(err)))?;

        for path in paths {
            ino.add_watch(&path, ffi::IN_ALL_EVENTS)
                .or_else(|err| Err(BackendError::Io(err)))?;
        }

        Ok(Backend { buffer: Buffer::new(), inotify: ino })
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

        let from_kernel = self.inotify.available_events()
            .or_else(|err| Err(StreamError::Io(err)))?;

        for e in from_kernel {
            if e.is_queue_overflow() {
                // Currently, futures::Stream don't terminate on Error, so we
                // close the buffer such that the rest of the events trickle
                // through and the stream ends with Ready(None) after all are
                // through. If futures::Stream change so they terminate on
                // Error, we'll need to change the Buffer so it may carry an
                // Error value, and output it at the end of the stream. In
                // either case, it's important that we do continue to provide
                // the received events, even in the case of an error/overflow
                // upstream.
                self.buffer.close();
                return Err(StreamError::UpstreamOverflow)
            }

            if e.is_ignored() {
                self.buffer.close();
                break
            }

            self.buffer.push(Event {
                kind: if e.is_access() {
                    EventKind::Access(AccessKind::Any)
                } else if e.is_attrib() {
                    EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any))
                } else if e.is_close_write() {
                    EventKind::Access(AccessKind::Close(AccessMode::Write))
                } else if e.is_close_nowrite() {
                    EventKind::Access(AccessKind::Close(AccessMode::Read))
                } else if e.is_create() {
                    EventKind::Create(if e.is_dir() {
                        CreateKind::Folder
                    } else {
                        CreateKind::File
                    })
                } else if e.is_delete() {
                    EventKind::Remove(if e.is_dir() {
                        RemoveKind::Folder
                    } else {
                        RemoveKind::File
                    })
                } else if e.is_delete_self() {
                    EventKind::Remove(if e.is_dir() {
                        RemoveKind::Folder
                    } else {
                        RemoveKind::File
                    })
                } else if e.is_modify() {
                    EventKind::Modify(ModifyKind::Data(DataChange::Any))
                } else if e.is_move_self() {
                    EventKind::Modify(ModifyKind::Name(RenameMode::Any))
                } else if e.is_moved_from() {
                    EventKind::Modify(ModifyKind::Name(RenameMode::From))
                } else if e.is_moved_to() {
                    EventKind::Modify(ModifyKind::Name(RenameMode::To))
                } else if e.is_open() {
                    EventKind::Access(AccessKind::Open(AccessMode::Any))
                } else if e.is_unmount() {
                    EventKind::Remove(RemoveKind::Other("unmount".into()))
                } else {
                    EventKind::Any
                },
                paths: vec![e.name.clone()],
                relid: match e.cookie {
                    0 => None,
                    c @ _ => Some(c as usize)
                }
            })
        }

        self.buffer.poll()
    }
}
