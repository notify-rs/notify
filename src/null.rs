#![allow(unused_variables)]

use std::sync::mpsc::Sender;
use std::path::Path;
use super::{Error, Event, Watcher};

pub struct NullWatcher;

impl Watcher for NullWatcher {
  fn new(tx: Sender<Event>) -> Result<NullWatcher, Error> {
    Ok(NullWatcher)
  }

  fn watch(&mut self, path: &Path) -> Result<(), Error> {
    Ok(())
  }

  fn unwatch(&mut self, path: &Path) -> Result<(), Error> {
    Ok(())
  }
}

