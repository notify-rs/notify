//! Generic Watcher implementation based on polling
//!
//! Checks the `watch`ed paths periodically to detect changes. This implementation only uses
//! cross-platform APIs; it should function on any platform that the Rust standard library does.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::sync::mpsc::Sender;
use std::fs;
use std::thread;
use super::{Error, Event, op, Result, Watcher, RecursiveMode};
use std::path::{Path, PathBuf};
use std::time::Duration;
use self::walkdir::WalkDir;

use filetime::FileTime;

extern crate walkdir;
extern crate time;

/// Polling based `Watcher` implementation
pub struct PollWatcher {
    tx: Sender<Event>,
    watches: Arc<RwLock<HashMap<PathBuf, bool>>>,
    open: Arc<RwLock<bool>>,
}

impl PollWatcher {
    /// Create a PollWatcher which polls every `delay` milliseconds
    pub fn with_delay(tx: Sender<Event>, delay: u32) -> Result<PollWatcher> {
        let mut p = PollWatcher {
            tx: tx,
            watches: Arc::new(RwLock::new(HashMap::new())),
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
            // TODO: handle chmod events
            // TODO: handle renames
            // TODO: DRY it up
            let mut mtimes: HashMap<PathBuf, (u64, f64)> = HashMap::new();

            let mut current_time = time::precise_time_s();

            for (watch, is_recursive) in watches.read().unwrap().iter() {
                match fs::metadata(watch) {
                    Err(e) => {
                        let _ = tx.send(Event {
                            path: Some(watch.clone()),
                            op: Err(Error::Io(e)),
                        });
                        continue;
                    }
                    Ok(metadata) => {
                        if !metadata.is_dir() {
                            let modified = FileTime::from_last_modification_time(&metadata).seconds();
                            mtimes.insert(watch.clone(), (modified, current_time));
                        } else {
                            let depth = if *is_recursive {
                                usize::max_value()
                            } else {
                                1
                            };
                            for entry in WalkDir::new(watch)
                                                .follow_links(true)
                                                .max_depth(depth)
                                                .into_iter()
                                                .filter_map(|e| e.ok()) {
                                let path = entry.path();

                                match entry.metadata() {
                                    Err(e) => {
                                        let _ = tx.send(Event {
                                            path: Some(path.to_path_buf()),
                                            op: Err(Error::Io(e.into())),
                                        });
                                    }
                                    Ok(m) => {
                                        let modified = FileTime::from_last_modification_time(&m).seconds();
                                        mtimes.insert(path.to_path_buf(), (modified, current_time));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            loop {
                if !(*open.read().unwrap()) {
                    break;
                }

                current_time = time::precise_time_s();

                for (watch, is_recursive) in watches.read().unwrap().iter() {
                    match fs::metadata(watch) {
                        Err(e) => {
                            let _ = tx.send(Event {
                                path: Some(watch.clone()),
                                op: Err(Error::Io(e)),
                            });
                            continue;
                        }
                        Ok(metadata) => {
                            if !metadata.is_dir() {
                                let modified = FileTime::from_last_modification_time(&metadata).seconds();
                                match mtimes.insert(watch.clone(), (modified, current_time)) {
                                    None => {
                                        unreachable!();
                                    }
                                    Some((old_modified, _)) => {
                                        if modified > old_modified {
                                            let _ = tx.send(Event {
                                                path: Some(watch.clone()),
                                                op: Ok(op::WRITE),
                                            });
                                        }
                                    }
                                }
                            } else {
                                let depth = if *is_recursive {
                                    usize::max_value()
                                } else {
                                    1
                                };
                                for entry in WalkDir::new(watch)
                                                    .follow_links(true)
                                                    .max_depth(depth)
                                                    .into_iter()
                                                    .filter_map(|e| e.ok()) {
                                    let path = entry.path();

                                    match entry.metadata() {
                                        Err(e) => {
                                            let _ = tx.send(Event {
                                                path: Some(path.to_path_buf()),
                                                op: Err(Error::Io(e.into())),
                                            });
                                        }
                                        Ok(m) => {
                                            let modified = FileTime::from_last_modification_time(&m).seconds();
                                            match mtimes.insert(path.to_path_buf(), (modified, current_time)) {
                                                None => {
                                                    let _ = tx.send(Event {
                                                        path: Some(path.to_path_buf()),
                                                        op: Ok(op::CREATE),
                                                    });
                                                }
                                                Some((old_modified, _)) => {
                                                    if modified > old_modified {
                                                        let _ = tx.send(Event {
                                                            path: Some(path.to_path_buf()),
                                                            op: Ok(op::WRITE),
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

                let mut removed: Vec<PathBuf> = Vec::new();

                'paths: for (ref path, &(_, last_checked)) in &mtimes {
                    for (watch, _) in watches.read().unwrap().iter() {
                        if path.starts_with(watch) {
                            if last_checked < current_time {
                                let _ = tx.send(Event {
                                    path: Some(path.to_path_buf()),
                                    op: Ok(op::REMOVE),
                                });
                                removed.push(path.to_path_buf());
                            }
                            continue 'paths;
                        }
                    }
                    // not found in watches
                    removed.push(path.to_path_buf());
                }

                for path in removed {
                    mtimes.remove(&path);
                }

                if delay != 0 {
                    thread::sleep(Duration::from_millis(delay as u64));
                }
            }
        });
    }
}

impl Watcher for PollWatcher {
    fn new(tx: Sender<Event>) -> Result<PollWatcher> {
        PollWatcher::with_delay(tx, 10)
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        (*self.watches).write().unwrap().insert(path.as_ref().to_path_buf(), recursive_mode.is_recursive());
        Ok(())
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        if (*self.watches).write().unwrap().remove(path.as_ref()).is_some() {
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
