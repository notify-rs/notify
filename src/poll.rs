//! Generic Watcher implementation based on polling
//!
//! Checks the `watch`ed paths periodically to detect changes. This implementation only uses
//! Rust stdlib APIs and should work on all of the platforms it supports.

use walkdir::WalkDir;
use super::debounce::EventTx;
use super::{op, Error, RawEvent, RecursiveMode, Result, Watcher};
use crossbeam_channel::Sender;
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
    event_tx: EventTx,
    watches: Arc<Mutex<HashMap<PathBuf, WatchData>>>,
    open: Arc<AtomicBool>,
    delay: Duration,
}

impl PollWatcher {
    /// Create a PollWatcher which polls every `delay` milliseconds
    pub fn with_delay(tx: Sender<RawEvent>, delay: Duration) -> Result<PollWatcher> {
        let event_tx = EventTx::new_immediate(tx);
        let mut p = PollWatcher {
            event_tx: event_tx.clone(),
            watches: Arc::new(Mutex::new(HashMap::new())),
            open: Arc::new(AtomicBool::new(true)),
            delay,
        };
        p.run(event_tx);
        Ok(p)
    }

    fn run(&mut self, event_tx: EventTx) {
        let watches = self.watches.clone();
        let open = self.open.clone();
        let delay = self.delay;

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
                                event_tx.send(RawEvent {
                                    path: Some(watch.clone()),
                                    op: Err(Error::io(e)),
                                    cookie: None,
                                });
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
                                                event_tx.send(RawEvent {
                                                    path: Some(watch.clone()),
                                                    op: Ok(op::Op::WRITE),
                                                    cookie: None,
                                                });
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
                                                event_tx.send(RawEvent {
                                                    path: Some(path.to_path_buf()),
                                                    op: Err(Error::io(e.into())),
                                                    cookie: None,
                                                });
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
                                                        event_tx.send(RawEvent {
                                                            path: Some(path.to_path_buf()),
                                                            op: Ok(op::Op::CREATE),
                                                            cookie: None,
                                                        });
                                                    }
                                                    Some(PathData {
                                                        mtime: old_mtime, ..
                                                    }) => {
                                                        if mtime > old_mtime {
                                                            event_tx.send(RawEvent {
                                                                path: Some(path.to_path_buf()),
                                                                op: Ok(op::Op::WRITE),
                                                                cookie: None,
                                                            });
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
                                event_tx.send(RawEvent {
                                    path: Some(path.clone()),
                                    op: Ok(op::Op::REMOVE),
                                    cookie: None,
                                });
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
}

impl Watcher for PollWatcher {
    fn new_immediate(tx: Sender<RawEvent>) -> Result<PollWatcher> {
        PollWatcher::with_delay(tx, Duration::from_secs(30))
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        if let Ok(mut watches) = self.watches.lock() {
            let current_time = Instant::now();

            let watch = path.as_ref().to_owned();

            match fs::metadata(path) {
                Err(e) => {
                    self.event_tx.send(RawEvent {
                        path: Some(watch.clone()),
                        op: Err(Error::io(e)),
                        cookie: None,
                    });
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
                                    self.event_tx.send(RawEvent {
                                        path: Some(path.to_path_buf()),
                                        op: Err(Error::io(e.into())),
                                        cookie: None,
                                    });
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

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        if (*self.watches)
            .lock()
            .unwrap()
            .remove(path.as_ref())
            .is_some()
        {
            Ok(())
        } else {
            Err(Error::watch_not_found())
        }
    }
}

impl Drop for PollWatcher {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Relaxed);
    }
}

// Because all public methods are `&mut self` it's also perfectly safe to share references.
unsafe impl Sync for PollWatcher {}
