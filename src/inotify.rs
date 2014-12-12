extern crate "inotify" as inotify_sys;
extern crate libc;

use self::libc::c_int;
use self::inotify_sys::wrapper::{mod, INotify, Watch};
use std::collections::HashMap;
use std::io::{IoError, IoErrorKind};
use std::sync::Arc;
use super::{Event, op, Op, Error, Watcher};

pub struct INotifyWatcher {
  inotify: INotify,
  tx: Sender<Event>,
  watches: HashMap<Path, Watch>,
  paths: HashMap<Watch, Path>
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
    });
  }
}

fn handle_event(event: wrapper::Event, tx: &Sender<Event>, paths: &HashMap<Watch, Path>) {
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
    true => match paths.get(&event.wd) {
      Some(p) => Some(p.clone()),
      None => None
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
        watches: HashMap::new(),
        paths: HashMap::new() // TODO: use bimap
      },
      Err(e) => return Err(Error::Io(e))
    };
    
    it.run();
    return Ok(it);
  }

  fn watch(&mut self, path: &Path) -> Result<(), Error> {
    Err(Error::NotImplemented)
  }

  fn unwatch(&mut self, path: &Path) -> Result<(), Error> {
    Err(Error::NotImplemented)
  }
}
