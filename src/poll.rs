use std::collections::{HashMap, HashSet};
use std::io::fs::{mod, PathExtensions};
use std::sync::{Arc, RWLock};
use std::thread::Thread;
use super::{Error, Event, op, Watcher};

pub struct PollWatcher {
  tx: Sender<Event>,
  watches: Arc<RWLock<HashSet<Path>>>,
  open: Arc<RWLock<bool>>
}

impl PollWatcher {
  fn run(&mut self) {
    let tx = self.tx.clone();
    let watches = self.watches.clone();
    let open = self.open.clone();
    Thread::spawn(move || {
      // In order of priority:
      // TODO: populate mtimes before loop, and then handle creation events
      // TODO: handle deletion events
      // TODO: handle chmod events
      // TODO: handle renames
      // TODO: DRY it up
      let mut mtimes: HashMap<Path, u64> = HashMap::new();
      loop {
        if !(*open.read().unwrap()) {
          break
        }

        for watch in watches.read().unwrap().iter() {
          if !watch.exists() {
            tx.send(Event {
              path: Some(watch.clone()),
              op: Err(Error::PathNotFound)
            });
            continue
          }

          match watch.lstat() {
            Err(e) => {
              tx.send(Event {
                path: Some(watch.clone()),
                op: Err(Error::Io(e))
              });
              continue
            },
            Ok(stat) => {
              match mtimes.insert(watch.clone(), stat.modified) {
                None => continue, // First run
                Some(old) => {
                  if stat.modified > old {
                    tx.send(Event {
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
              tx.send(Event {
                path: Some(watch.clone()),
                op: Err(Error::Io(e))
              });
              continue
            },
            Ok(mut iter) => {
              for path in iter {
                match path.lstat() {
                  Err(e) => {
                    tx.send(Event {
                      path: Some(path.clone()),
                      op: Err(Error::Io(e))
                    });
                    continue
                  },
                  Ok(stat) => {
                    match mtimes.insert(path.clone(), stat.modified) {
                      None => continue, // First run
                      Some(old) => {
                        if stat.modified > old {
                          tx.send(Event {
                            path: Some(path.clone()),
                            op: Ok(op::WRITE)
                          });
                          continue
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
    }).detach();
  }
}

impl Watcher for PollWatcher {
  fn new(tx: Sender<Event>) -> Result<PollWatcher, Error> {
    let mut p = PollWatcher {
      tx: tx,
      watches: Arc::new(RWLock::new(HashSet::new())),
      open: Arc::new(RWLock::new(true))
    };
    p.run();
    Ok(p)
  }

  fn watch(&mut self, path: &Path) -> Result<(), Error> {
    (*self.watches).write().unwrap().insert(path.clone());
    Ok(())
  }

  fn unwatch(&mut self, path: &Path) -> Result<(), Error> {
    if (*self.watches).write().unwrap().remove(path) {
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
