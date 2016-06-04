//! Generic Watcher implementation based on polling
//!
//! Checks the `watch`ed paths periodically to detect changes. This implementation only uses
//! cross-platform APIs; it should function on any platform that the Rust standard library does.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::sync::mpsc::Sender;
use std::fs;
use std::thread;
use super::{Error, Event, op, Result, Watcher};
use std::path::{Path, PathBuf};
use std::time::Duration;
use self::walkdir::WalkDir;

use filetime::FileTime;

extern crate walkdir;

/// Polling based `Watcher` implementation
pub struct PollWatcher {
    tx: Sender<Event>,
    watches: Arc<RwLock<HashSet<PathBuf>>>,
    open: Arc<RwLock<bool>>,
}

impl PollWatcher {
    /// Create a PollWatcher which polls every `delay` milliseconds
    pub fn with_delay(tx: Sender<Event>, delay: u32) -> Result<PollWatcher> {
        let mut p = PollWatcher {
            tx: tx,
            watches: Arc::new(RwLock::new(HashSet::new())),
            open: Arc::new(RwLock::new(true)),
        };
        p.run(delay);
        Ok(p)
    }

    fn run(&mut self, delay: u32) {
        let tx = self.tx.clone();
        let watches = self.watches.clone();
        let open = self.open.clone();
        thread::spawn(move || {
            // In order of priority:
            // TODO: populate mtimes before loop, and then handle creation events
            // TODO: handle deletion events
            // TODO: handle chmod events
            // TODO: handle renames
            // TODO: DRY it up
            let mut mtimes: HashMap<PathBuf, u64> = HashMap::new();
            loop {
                if delay != 0 {
                    thread::sleep(Duration::from_millis(delay as u64));
                }
                if !(*open.read().unwrap()) {
                    break;
                }

                for watch in watches.read().unwrap().iter() {
                    let meta = fs::metadata(watch);

                    if !meta.is_ok() {
                        let _ = tx.send(Event {
                            path: Some(watch.clone()),
                            op: Err(Error::PathNotFound),
                        });
                        continue;
                    }

                    match meta {
                        Err(e) => {
                            let _ = tx.send(Event {
                                path: Some(watch.clone()),
                                op: Err(Error::Io(e)),
                            });
                            continue;
                        }
                        Ok(stat) => {
                            let modified = FileTime::from_last_modification_time(&stat).seconds();

                            match mtimes.insert(watch.clone(), modified) {
                                None => continue, // First run
                                Some(old) => {
                                    if modified > old {
                                        let _ = tx.send(Event {
                                            path: Some(watch.clone()),
                                            op: Ok(op::WRITE),
                                        });
                                        continue;
                                    }
                                }
                            }

                            if !stat.is_dir() {
                                continue;
                            }
                        }
                    }

                    // TODO: more efficient implementation where the dir tree is cached?
                    for entry in WalkDir::new(watch)
                                     .follow_links(true)
                                     .into_iter()
                                     .filter_map(|e| e.ok()) {
                        let path = entry.path();

                        match fs::metadata(&path) {
                            Err(e) => {
                                let _ = tx.send(Event {
                                    path: Some(path.to_path_buf()),
                                    op: Err(Error::Io(e)),
                                });
                                continue;
                            }
                            Ok(stat) => {
                                let modified = FileTime::from_last_modification_time(&stat)
                                                   .seconds();
                                match mtimes.insert(path.to_path_buf(), modified) {
                                    None => continue, // First run
                                    Some(old) => {
                                        if modified > old {
                                            let _ = tx.send(Event {
                                                path: Some(path.to_path_buf()),
                                                op: Ok(op::WRITE),
                                            });
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}

impl Watcher for PollWatcher {
    fn new(tx: Sender<Event>) -> Result<PollWatcher> {
        PollWatcher::with_delay(tx, 10)
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        (*self.watches).write().unwrap().insert(path.as_ref().to_path_buf());
        Ok(())
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        if (*self.watches).write().unwrap().remove(path.as_ref()) {
            Ok(())
        } else {
            Err(Error::WatchNotFound)
        }
    }
}

impl Drop for PollWatcher {
    fn drop(&mut self) {
        {
            let mut open = (*self.open).write().unwrap();
            (*open) = false;
        }
    }
}
