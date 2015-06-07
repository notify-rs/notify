extern crate inotify as inotify_sys;
extern crate libc;
extern crate walker;

use self::inotify_sys::wrapper::{self, INotify, Watch};
use self::walker::Walker;
use std::collections::HashMap;
use std::fs::metadata;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread;
use super::{Error, Event, op, Op, Watcher};

mod flags;

pub struct INotifyWatcher {
  inotify: INotify,
  tx: Sender<Event>,
  watches: HashMap<PathBuf, (Watch, flags::Mask)>,
  paths: Arc<RwLock<HashMap<Watch, PathBuf>>>
}

impl INotifyWatcher {
  fn run(&mut self) {
    let mut ino = self.inotify.clone();
    let tx = self.tx.clone();
    let paths = self.paths.clone();
    thread::spawn(move || {
      loop {
        match ino.wait_for_events() {
          Ok(es) => {
            for e in es.iter() {
              handle_event(e.clone(), &tx, &paths)
            }
          },
          Err(e) => {
            match e.kind() {
              _ => {
                let _ = tx.send(Event {
                  path: None,
                  op: Err(Error::Io(e))
                });
              }
            }
          }
        }
      }
    });
  }

  fn add_watch(&mut self, path: &Path) -> Result<(), Error> {
    let mut watching  = flags::IN_ATTRIB
                      | flags::IN_CREATE
                      | flags::IN_DELETE
                      | flags::IN_DELETE_SELF
                      | flags::IN_MODIFY
                      | flags::IN_MOVED_FROM
                      | flags::IN_MOVED_TO
                      | flags::IN_MOVE_SELF;
    let path = path.to_path_buf();
    match self.watches.get(&path) {
      None => {},
      Some(p) => {
        watching.insert((&p.1).clone());
        watching.insert(flags::IN_MASK_ADD);
      }
    }

    match self.inotify.add_watch(&path, watching.bits()) {
      Err(e) => return Err(Error::Io(e)),
      Ok(w) => {
        watching.remove(flags::IN_MASK_ADD);
        self.watches.insert(path.clone(), (w.clone(), watching));
        (*self.paths).write().unwrap().insert(w.clone(), path);
        Ok(())
      }
    }
  }
}

#[inline]
fn handle_event(event: wrapper::Event, tx: &Sender<Event>, paths: &Arc<RwLock<HashMap<Watch, PathBuf>>>) {
  let mut o = Op::empty();
  if event.is_create() || event.is_moved_to() {
    o.insert(op::CREATE);
  }
  if event.is_delete_self() || event.is_delete() {
    o.insert(op::WRITE);
  }
  if event.is_modify() {
    o.insert(op::REMOVE);
  }
  if event.is_move_self() || event.is_moved_from() {
    o.insert(op::RENAME);
  }
  if event.is_attrib() {
    o.insert(op::CHMOD);
  }

  let path = match event.name.is_empty() {
    true => {
      match (*paths).read().unwrap().get(&event.wd) {
        Some(p) => Some(p.clone()),
        None => None
      }
    },
    false => paths.read().unwrap().get(&event.wd).map(|root| root.join(&event.name)),
  };

  let _ = tx.send(Event {
    path: path,
    op: Ok(o)
  });
}

impl Watcher for INotifyWatcher {
  fn new(tx: Sender<Event>) -> Result<INotifyWatcher, Error> {
    let mut it = match INotify::init() {
      Ok(i) => INotifyWatcher {
        inotify: i,
        tx: tx,
        watches: HashMap::new(), // TODO: use bimap?
        paths: Arc::new(RwLock::new(HashMap::new()))
      },
      Err(e) => return Err(Error::Io(e))
    };

    it.run();
    return Ok(it);
  }

  fn watch(&mut self, path: &Path) -> Result<(), Error> {
    match Walker::new(path) {
      Ok(d) => {
        for dir in d {
          match dir {
            Ok(entry) => {
              let path = entry.path();
              let meta = match metadata(&path) {
                Ok(m) => m,
                Err(e) => return Err(Error::Io(e)),
              };

              if meta.is_dir() {
                try!(self.add_watch(&path));
              }
            },
            Err(e) => return Err(Error::Io(e)),
          }
        }

        self.add_watch(path)
      },
      Err(e) => Err(Error::Io(e))
    }
  }

  fn unwatch(&mut self, path: &Path) -> Result<(), Error> {
    // FIXME:
    // once Rust 1.1 is released, just use a &Path
    // Relevant bug is https://github.com/rust-lang/rust/pull/25060
    match self.watches.remove(&path.to_path_buf()) {
      None => Err(Error::WatchNotFound),
      Some(p) => {
        let w = &p.0;
        match self.inotify.rm_watch(w.clone()) {
          Err(e) => Err(Error::Io(e)),
          Ok(_) => {
            // Nothing depends on the value being gone
            // from here now that inotify isn't watching.
            (*self.paths).write().unwrap().remove(w);
            Ok(())
          }
        }
      }
    }
  }
}

impl Drop for INotifyWatcher {
  fn drop(&mut self) {
    for path in self.watches.clone().keys() {
      let _ = self.unwatch(path);
    }
    let _ = self.inotify.close();
  }
}
