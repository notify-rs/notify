#![forbid(unsafe_code)]
#![cfg_attr(feature = "cargo-clippy", deny(clippy_pedantic))]

extern crate filetime;
extern crate id_tree;
extern crate notify_backend as backend;
extern crate walkdir;

mod poll_thread;

use poll_thread::poll_thread;

use backend::prelude::*;
use backend::Buffer;
use futures::task::{self, Task};
use futures::{Poll, Stream};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

const BACKEND_NAME: &str = "poll tree";

#[derive(Debug)]
pub struct Backend {
    poll_thread: Option<thread::JoinHandle<()>>,
    task: Option<Task>,
    buffer: Buffer,
    event_rx: mpsc::Receiver<io::Result<Event>>,
    task_tx: mpsc::Sender<Task>,
    shutdown_tx: mpsc::Sender<()>,
    registration: Arc<MioRegistration>,
    readiness: MioReadiness,
}

impl NotifyBackend for Backend {
    fn name() -> &'static str {
        BACKEND_NAME
    }

    fn new(paths: Vec<PathBuf>) -> NewBackendResult {
        let interval = Duration::from_millis(20);
        let (event_tx, event_rx) = mpsc::channel();
        let (task_tx, task_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let (reg, readiness) = MioRegistration::new2();

        let ready = readiness.clone();
        let poll_thread = Some(thread::spawn(move || {
            poll_thread(paths, interval, event_tx, task_rx, shutdown_rx, ready);
        }));

        Ok(Box::new(Backend {
            poll_thread,
            task: None,
            buffer: Buffer::new(),
            event_rx,
            task_tx,
            shutdown_tx, // TODO use oneshot
            registration: Arc::new(reg),
            readiness,
        }))
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

    fn driver(&self) -> Box<Evented> {
        Box::new(self.registration.clone())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // send shutdown signal to thread
        if self.shutdown_tx.send(()).is_ok() {
            if let Some(poll_thread) = self.poll_thread.take() {
                // wake up thread
                poll_thread.thread().unpark();
                // wait for the thread to exit
                let _ = poll_thread.join();
            }
        }
    }
}

impl Stream for Backend {
    type Item = StreamItem;
    type Error = StreamError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.buffer.closed() {
            return self.buffer.poll();
        }

        if !self
            .task
            .as_ref()
            .map(|t| t.will_notify_current())
            .unwrap_or(false)
        {
            let task = task::current();
            self.task = Some(task.clone());
            let _ = self.task_tx.send(task);
        }

        loop {
            match self.event_rx.try_recv() {
                Ok(event) => self.buffer.push(event?),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(_) => {
                    return Err(StreamError::Io(io::Error::new(
                        io::ErrorKind::Other,
                        "poll thread crashed",
                    )))
                }
            }
        }

        self.buffer.poll()
    }
}

#[cfg(test)]
mod tests {
    extern crate tempdir;

    use super::Backend as PollBackend;

    use self::tempdir::TempDir;
    use backend::backend::Backend;

    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn shutdown_within_10ms() {
        let dir = TempDir::new("watch_folder").expect("create tmp dir");
        let path = dir.path().to_path_buf();

        for i in 0..10u32 {
            let start = {
                let _backend = PollBackend::new(vec![path.clone()]).expect("init backend");
                thread::sleep(Duration::from_millis(u64::from(i * 5)));
                Instant::now()
            };

            let duration_since_start = Instant::now().duration_since(start);
            assert!(duration_since_start.subsec_nanos() < 10_000_000);
        }
    }
}
