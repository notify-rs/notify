use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::sync::mpsc::Sender;
use std::fs;
use std::thread;
use super::{Error, Event, op, Watcher};
use std::path::{Path, PathBuf};
use self::walker::Walker;

use filetime::FileTime;

extern crate walker;

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
          let meta = fs::metadata(watch);

          if !meta.is_ok() {
            let _ = tx.send(Event {
              path: Some(watch.clone()),
              op: Err(Error::PathNotFound)
            });
            continue
          }

          match meta {
            Err(e) => {
              let _ = tx.send(Event {
                path: Some(watch.clone()),
                op: Err(Error::Io(e))
              });
              continue
            },
            Ok(stat) => {
              let modified =
                FileTime::from_last_modification_time(&stat)
                .seconds();

              match mtimes.insert(watch.clone(), modified) {
                None => continue, // First run
                Some(old) => {
                  if modified > old {
                    let _ = tx.send(Event {
                      path: Some(watch.clone()),
                      op: Ok(op::WRITE)
                    });
                    continue
                  }
                }
              }

              if !stat.is_dir() { continue }
            }
          }

          // TODO: more efficient implementation where the dir tree is cached?
          match Walker::new(watch) {
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

                    match fs::metadata(&path) {
                      Err(e) => {
                        let _ = tx.send(Event {
                          path: Some(path.clone()),
                          op: Err(Error::Io(e))
                        });
                        continue
                      },
                      Ok(stat) => {
                        let modified =
                          FileTime::from_last_modification_time(&stat)
                          .seconds();

                        match mtimes.insert(path.clone(), modified) {
                          None => continue, // First run
                          Some(old) => {
                            if modified > old {
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

  fn watch<P: AsRef<Path> + ?Sized>(&mut self, path: &P) -> Result<(), Error> {
    (*self.watches).write().unwrap().insert(path.as_ref().to_path_buf());
    Ok(())
  }

  fn unwatch<P: AsRef<Path> + ?Sized>(&mut self, path: &P) -> Result<(), Error> {
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
