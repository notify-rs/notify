//! Generic Watcher implementation based on polling
//!
//! Checks the `watch`ed paths periodically to detect changes. This implementation only uses
//! Rust stdlib APIs and should work on all of the platforms it supports.

use super::event::*;
use super::{Error, EventFn, RecursiveMode, Result, Watcher};
use filetime::FileTime;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

struct PathData {
    mtime: i64,
    last_check: Instant,
}

struct WatchData {
    is_recursive: bool,
    paths: HashMap<PathBuf, PathData>,
}

/// Polling based `Watcher` implementation
pub struct PollWatcher {
    event_fn: Arc<Mutex<dyn EventFn>>,
    watches: Arc<Mutex<HashMap<PathBuf, WatchData>>>,
    open: Arc<AtomicBool>,
    delay: Duration,
}

fn emit_event(event_fn: &Mutex<dyn EventFn>, res: Result<Event>) {
    if let Ok(guard) = event_fn.lock() {
        let f: &dyn EventFn = &*guard;
        f(res);
    }
}

impl PollWatcher {
    /// Create a PollWatcher which polls every `delay` milliseconds
    pub fn with_delay(event_fn: Arc<Mutex<dyn EventFn>>, delay: Duration) -> Result<PollWatcher> {
        let mut p = PollWatcher {
            event_fn,
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
        let event_fn = self.event_fn.clone();
        let event_fn = move |res| emit_event(&event_fn, res);

        thread::spawn(move || {
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
                                event_fn(err);
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
                                                event_fn(Ok(ev));
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
                                                event_fn(Err(err));
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
                                                        event_fn(Ok(ev));
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
                                                            event_fn(Ok(ev));
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
                                event_fn(Ok(ev));
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
                    let err = Error::io(e).add_path(watch.clone());
                    emit_event(&self.event_fn, Err(err));
                }
                Ok(metadata) => {
                    if !metadata.is_dir() {
                        let mut paths = HashMap::new();
                        let mtime = FileTime::from_last_modification_time(&metadata).seconds();
                        paths.insert(
                            watch.clone(),
                            PathData {
                                mtime,
                                last_check: current_time,
                            },
                        );
                        watches.insert(
                            watch,
                            WatchData {
                                is_recursive: recursive_mode.is_recursive(),
                                paths,
                            },
                        );
                    } else {
                        let mut paths = HashMap::new();
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
                                    emit_event(&self.event_fn, Err(err));
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
    fn new_immediate<F: EventFn>(event_fn: F) -> Result<PollWatcher> {
        let event_fn = Arc::new(Mutex::new(event_fn));
        let delay = Duration::from_secs(30);
        PollWatcher::with_delay(event_fn, delay)
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        self.watch_inner(path.as_ref(), recursive_mode)
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        self.unwatch_inner(path.as_ref())
    }
}

impl Drop for PollWatcher {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Relaxed);
    }
}

// Because all public methods are `&mut self` it's also perfectly safe to share references.
unsafe impl Sync for PollWatcher {}
