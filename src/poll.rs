use std::collections::{HashMap, HashSet};
use std::io::fs::PathExtensions;
use std::sync::{Arc, RWLock};
use super::{Error, Event, op, Op, Watcher};

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
    spawn(proc() {
      let mut mtimes: HashMap<Path, u64> = HashMap::new();
      loop {
        if !(*open.read()) {
          break
        }

        for watch in watches.read().iter() {
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
        }
      }
    })
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
    (*self.watches).write().insert(path.clone());
    Ok(())
  }

  fn unwatch(&mut self, path: &Path) -> Result<(), Error> {
    if (*self.watches).write().remove(path) {
      Ok(())
    } else {
      Err(Error::WatchNotFound)
    }
  }
}

impl Drop for PollWatcher {
  fn drop(&mut self) {
    {
      let mut open = (*self.open).write();
      (*open) = false;
    }
  }
}
