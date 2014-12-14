extern crate "inotify" as inotify_sys;
extern crate libc;

use self::inotify_sys::wrapper::{mod, INotify, Watch};
use std::collections::HashMap;
use std::io::IoErrorKind;
use std::sync::{Arc, RWLock};
use super::{Error, Event, op, Op, Watcher};

mod flags;

pub struct INotifyWatcher {
  inotify: INotify,
  tx: Sender<Event>,
  watches: HashMap<Path, (Watch, flags::Mask)>,
  paths: Arc<RWLock<HashMap<Watch, Path>>>
}

impl INotifyWatcher {
  fn run(&mut self) {
    let mut ino = self.inotify.clone();
    let tx = self.tx.clone();
    let paths = self.paths.clone();
    spawn(proc() {
      loop {
        match ino.event() {
          Ok(e) => {
            handle_event(e, &tx, &paths)
          },
          Err(e) => {
            match e.kind {
              IoErrorKind::EndOfFile => break,
              _ => tx.send(Event {
                path: None,
                op: Err(Error::Io(e))
              })
            }
          }
        }
      }
    })
  }
}

#[inline]
fn handle_event(event: wrapper::Event, tx: &Sender<Event>, paths: &Arc<RWLock<HashMap<Watch, Path>>>) {
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
      match (*paths).read().get(&event.wd) {
        Some(p) => Some(p.clone()),
        None => None
      }
    },
    false => Path::new_opt(event.name)
  };

  tx.send(Event {
    path: path,
    op: Ok(o)
  })
}

impl Watcher for INotifyWatcher {
  fn new(tx: Sender<Event>) -> Result<INotifyWatcher, Error> {
    let mut it = match INotify::init() {
      Ok(i) => INotifyWatcher {
        inotify: i,
        tx: tx,
        watches: HashMap::new(), // TODO: use bimap?
        paths: Arc::new(RWLock::new(HashMap::new()))
      },
      Err(e) => return Err(Error::Io(e))
    };
    
    it.run();
    return Ok(it);
  }

  fn watch(&mut self, path: &Path) -> Result<(), Error> {
    let mut watching  = flags::IN_ATTRIB
                      | flags::IN_CREATE
                      | flags::IN_DELETE
                      | flags::IN_DELETE_SELF
                      | flags::IN_MODIFY
                      | flags::IN_MOVED_FROM
                      | flags::IN_MOVED_TO
                      | flags::IN_MOVE_SELF;
    match self.watches.get(path) {
      None => {},
      Some(p) => {
        watching.insert(p.ref1().clone());
        watching.insert(flags::IN_MASK_ADD);
      }
    }

    match self.inotify.add_watch(path, watching.bits()) {
      Err(e) => return Err(Error::Io(e)),
      Ok(w) => {
        watching.remove(flags::IN_MASK_ADD);
        self.watches.insert(path.clone(), (w.clone(), watching));
        (*self.paths).write().insert(w.clone(), path.clone());
        Ok(())
      }
    }
  }

  fn unwatch(&mut self, path: &Path) -> Result<(), Error> {
    match self.watches.remove(path) {
      None => Err(Error::WatchNotFound),
      Some(p) => {
        let w = p.ref0();
        match self.inotify.rm_watch(w.clone()) {
          Err(e) => Err(Error::Io(e)),
          Ok(_) => {
            // Nothing depends on the value being gone
            // from here now that inotify isn't watching.
            (*self.paths).write().remove(w);
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
