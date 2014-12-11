extern crate "inotify" as ffi;

use self::ffi::wrapper::{mod, INotify, Watch};
use std::collections::HashMap;
use std::io::{IoError, IoErrorKind};
use std::sync::Arc;
use super::{Event, Op, Error, Watcher};

pub struct INotifyWatcher {
  inotify: INotify,
  tx: Sender<Event>,
  watches: HashMap<Path, Watch>,
}

impl INotifyWatcher {
  fn run(&mut self) {
    let mut ino = self.inotify.clone();
    let tx = self.tx.clone();
    spawn(proc() {
      loop {
        match ino.event() {
          Ok(e) => handle_event(e, &tx),
          Err(e) => {
            match e.kind {
              IoErrorKind::EndOfFile => break,
              _ => handle_error(e, &tx)
            }
          }
        }
      }
    });
  }
}

fn handle_event(event: wrapper::Event, tx: &Sender<Event>) {
  // TODO: translate wrapper::Event into Event
}

fn handle_error(error: IoError, tx: &Sender<Event>) {
  // TODO: handle all kinds of IoError
}

impl Watcher for INotifyWatcher {
  fn new(tx: Sender<Event>) -> Result<INotifyWatcher, Error> {
    let mut it = match INotify::init() {
      Ok(i) => INotifyWatcher {
        inotify: i,
        tx: tx,
        watches: HashMap::new()
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
