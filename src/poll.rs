use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::sync::mpsc::Sender;
use std::thread;
use super::{Error, Event, op, Watcher};
use std::fs::{self, PathExt};
use std::path::{Path, PathBuf};

pub struct PollWatcher {
  tx: Sender<Event>,
  watches: Arc<RwLock<HashSet<PathBuf>>>,
  open: Arc<RwLock<bool>>
}

impl PollWatcher {
  fn run(&mut self) {
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
        if !(*open.read().unwrap()) {
          break
        }

        for watch in watches.read().unwrap().iter() {
          if !watch.exists() {
            let _ = tx.send(Event {
              path: Some(watch.clone()),
              op: Err(Error::PathNotFound)
            });
            continue
          }

          match watch.metadata() {
            Err(e) => {
              let _ = tx.send(Event {
                path: Some(watch.clone()),
                op: Err(Error::Io(e))
              });
              continue
            },
            Ok(stat) => {
              match mtimes.insert(watch.clone(), stat.modified()) {
                None => continue, // First run
                Some(old) => {
                  if stat.modified() > old {
                    let _ = tx.send(Event {
                      path: Some(watch.clone()),
                      op: Ok(op::WRITE)
                    });
                    continue
                  }
                }
              }
            }
          }

          // TODO: recurse into the dir
          // TODO: more efficient implementation where the dir tree is cached?
          match fs::walk_dir(watch) {
            Err(e) => {
              let _ = tx.send(Event {
                path: Some(watch.clone()),
                op: Err(Error::Io(e))
              });
              continue
            },
            Ok(iter) => {
              for entry in iter {
                match entry {
                  Ok(entry) => {
                    let path = entry.path();

                    match path.metadata() {
                      Err(e) => {
                        let _ = tx.send(Event {
                          path: Some(path.clone()),
                          op: Err(Error::Io(e))
                        });
                        continue
                      },
                      Ok(stat) => {
                        match mtimes.insert(path.clone(), stat.modified()) {
                          None => continue, // First run
                          Some(old) => {
                            if stat.modified() > old {
                              let _ = tx.send(Event {
                                path: Some(path.clone()),
                                op: Ok(op::WRITE)
                              });
                              continue
                            }
                          }
                        }
                      }
                    }
                  },
                  Err(e) => {
                    let _ = tx.send(Event {
                      path: Some(watch.clone()),
                      op: Err(Error::Io(e))
                    });
                  },
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
  fn new(tx: Sender<Event>) -> Result<PollWatcher, Error> {
    let mut p = PollWatcher {
      tx: tx,
      watches: Arc::new(RwLock::new(HashSet::new())),
      open: Arc::new(RwLock::new(true))
    };
    p.run();
    Ok(p)
  }

  fn watch(&mut self, path: &Path) -> Result<(), Error> {
    // FIXME:
    // once https://github.com/rust-lang/rust/pull/22351 gets merged,
    // just use a &Path
    (*self.watches).write().unwrap().insert(path.to_path_buf());
    Ok(())
  }

  fn unwatch(&mut self, path: &Path) -> Result<(), Error> {
    // FIXME:
    // once https://github.com/rust-lang/rust/pull/22351 gets merged,
    // just use a &Path
    if (*self.watches).write().unwrap().remove(&path.to_path_buf()) {
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
