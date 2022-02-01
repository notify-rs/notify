//! Generic Watcher implementation based on polling
//!
//! Checks the `watch`ed paths periodically to detect changes. This implementation only uses
//! Rust stdlib APIs and should work on all of the platforms it supports.

use super::event::*;
use super::{Error, EventHandler, RecursiveMode, Result, Watcher};
use filetime::FileTime;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

#[derive(Debug)]
struct PathData {
    mtime: i64,
    last_check: Instant,
}

#[derive(Debug)]
struct WatchData {
    is_recursive: bool,
    paths: HashMap<PathBuf, PathData>,
}

/// Polling based `Watcher` implementation
pub struct PollWatcher {
    event_handler: Arc<Mutex<dyn EventHandler>>,
    watches: Arc<Mutex<HashMap<PathBuf, WatchData>>>,
    open: Arc<AtomicBool>,
    delay: Duration,
}

impl Debug for PollWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PollWatcher")
            .field("event_handler", &Arc::as_ptr(&self.watches))
            .field("watches", &self.watches)
            .field("open", &self.open)
            .field("delay", &self.delay)
            .finish()
    }
}

fn emit_event(event_handler: &Mutex<dyn EventHandler>, res: Result<Event>) {
    if let Ok(mut guard) = event_handler.lock() {
        let f: &mut dyn EventHandler = &mut *guard;
        f.handle_event(res);
    }
}

impl PollWatcher {
    /// Create a new [PollWatcher] and set the poll frequency to `delay`.
    pub fn with_delay<F: EventHandler>(event_handler: F, delay: Duration) -> Result<PollWatcher> {
        let mut p = PollWatcher {
            event_handler: Arc::new(Mutex::new(event_handler)),
            watches: Arc::new(Mutex::new(HashMap::new())),
            open: Arc::new(AtomicBool::new(true)),
            delay,
        };
        p.run();
        Ok(p)
    }

    fn run(&mut self) {
        let watches = self.watches.clone();
        let open = self.open.clone();
        let delay = self.delay;
        let event_handler = self.event_handler.clone();
        let event_handler = move |res| emit_event(&event_handler, res);

        thread::Builder::new().name("notify-rs poll".to_string()).spawn(move || {
            // In order of priority:
            // TODO: handle metadata events
            // TODO: handle renames
            // TODO: DRY it up

            loop {
                if !open.load(Ordering::SeqCst) {
                    break;
                }

                if let Ok(mut watches) = watches.lock() {
                    let current_time = Instant::now();

                    for (
                        watch,
                        &mut WatchData {
                            is_recursive,
                            ref mut paths,
                        },
                    ) in watches.iter_mut()
                    {
                        match fs::metadata(watch) {
                            Err(e) => {
                                let err = Err(Error::io(e).add_path(watch.clone()));
                                event_handler(err);
                                continue;
                            }
                            Ok(metadata) => {
                                if !metadata.is_dir() {
                                    let mtime =
                                        FileTime::from_last_modification_time(&metadata).seconds();
                                    match paths.insert(
                                        watch.clone(),
                                        PathData {
                                            mtime,
                                            last_check: current_time,
                                        },
                                    ) {
                                        None => {
                                            unreachable!();
                                        }
                                        Some(PathData {
                                            mtime: old_mtime, ..
                                        }) => {
                                            if mtime > old_mtime {
                                                let kind = MetadataKind::WriteTime;
                                                let meta = ModifyKind::Metadata(kind);
                                                let kind = EventKind::Modify(meta);
                                                let ev = Event::new(kind).add_path(watch.clone());
                                                event_handler(Ok(ev));
                                            }
                                        }
                                    }
                                } else {
                                    let depth = if is_recursive { usize::max_value() } else { 1 };
                                    for entry in WalkDir::new(watch)
                                        .follow_links(true)
                                        .max_depth(depth)
                                        .into_iter()
                                        .filter_map(|e| e.ok())
                                    {
                                        let path = entry.path();

                                        match entry.metadata() {
                                            Err(e) => {
                                                let err = Error::io(e.into())
                                                    .add_path(path.to_path_buf());
                                                event_handler(Err(err));
                                            }
                                            Ok(m) => {
                                                let mtime =
                                                    FileTime::from_last_modification_time(&m)
                                                        .seconds();
                                                match paths.insert(
                                                    path.to_path_buf(),
                                                    PathData {
                                                        mtime,
                                                        last_check: current_time,
                                                    },
                                                ) {
                                                    None => {
                                                        let kind =
                                                            EventKind::Create(CreateKind::Any);
                                                        let ev = Event::new(kind)
                                                            .add_path(path.to_path_buf());
                                                        event_handler(Ok(ev));
                                                    }
                                                    Some(PathData {
                                                        mtime: old_mtime, ..
                                                    }) => {
                                                        if mtime > old_mtime {
                                                            let kind = MetadataKind::WriteTime;
                                                            let meta = ModifyKind::Metadata(kind);
                                                            let kind = EventKind::Modify(meta);
                                                            // TODO add new mtime as attr
                                                            let ev = Event::new(kind)
                                                                .add_path(path.to_path_buf());
                                                            event_handler(Ok(ev));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    for (_, &mut WatchData { ref mut paths, .. }) in watches.iter_mut() {
                        let mut removed = Vec::new();
                        for (path, &PathData { last_check, .. }) in paths.iter() {
                            if last_check < current_time {
                                let ev = Event::new(EventKind::Remove(RemoveKind::Any))
                                    .add_path(path.clone());
                                event_handler(Ok(ev));
                                removed.push(path.clone());
                            }
                        }
                        for path in removed {
                            (*paths).remove(&path);
                        }
                    }
                }

                thread::sleep(delay);
            }
        });
    }

    fn watch_inner(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        if let Ok(mut watches) = self.watches.lock() {
            let current_time = Instant::now();

            let watch = path.to_owned();

            match fs::metadata(path) {
                Err(e) => {
                    let err = Error::io(e).add_path(watch);
                    emit_event(&self.event_handler, Err(err));
                }
                Ok(metadata) => {
                    let mut paths = HashMap::new();

                    if !metadata.is_dir() {
                        let mtime = FileTime::from_last_modification_time(&metadata).seconds();
                        paths.insert(
                            watch.clone(),
                            PathData {
                                mtime,
                                last_check: current_time,
                            },
                        );
                    } else {
                        let depth = if recursive_mode.is_recursive() {
                            usize::max_value()
                        } else {
                            1
                        };
                        for entry in WalkDir::new(watch.clone())
                            .follow_links(true)
                            .max_depth(depth)
                            .into_iter()
                            .filter_map(|e| e.ok())
                        {
                            let path = entry.path();

                            match entry.metadata() {
                                Err(e) => {
                                    let err = Error::io(e.into()).add_path(path.to_path_buf());
                                    emit_event(&self.event_handler, Err(err));
                                }
                                Ok(m) => {
                                    let mtime = FileTime::from_last_modification_time(&m).seconds();
                                    paths.insert(
                                        path.to_path_buf(),
                                        PathData {
                                            mtime,
                                            last_check: current_time,
                                        },
                                    );
                                }
                            }
                        }
                    }

                    watches.insert(
                        watch,
                        WatchData {
                            is_recursive: recursive_mode.is_recursive(),
                            paths,
                        },
                    );
                }
            }
        }
        Ok(())
    }

    fn unwatch_inner(&mut self, path: &Path) -> Result<()> {
        if (*self.watches).lock().unwrap().remove(path).is_some() {
            Ok(())
        } else {
            Err(Error::watch_not_found())
        }
    }
}

impl Watcher for PollWatcher {
    /// Create a new [PollWatcher].
    ///
    /// The default poll frequency is 30 seconds.
    /// Use [with_delay] to manually set the poll frequency.
    fn new<F: EventHandler>(event_handler: F) -> Result<Self> {
        let delay = Duration::from_secs(30);
        Self::with_delay(event_handler, delay)
    }

    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        self.watch_inner(path, recursive_mode)
    }

    fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.unwatch_inner(path)
    }

    fn kind() -> crate::WatcherKind {
        crate::WatcherKind::PollWatcher
    }
}

impl Drop for PollWatcher {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Relaxed);
    }
}

#[test]
fn poll_watcher_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<PollWatcher>();
}
