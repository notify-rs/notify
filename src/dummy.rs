use std::sync::mpsc::Sender;
use std::path::{Path};
use super::{Error, Event, Watcher};


pub struct DummyWatcher;

impl Watcher for DummyWatcher {
  fn new(_tx: Sender<Event>) -> Result<DummyWatcher, Error> {
    Err(Error::NotImplemented)
  }

  fn watch(&mut self, _path: &Path) -> Result<(), Error> {
    Err(Error::NotImplemented)
  }

  fn unwatch(&mut self, _path: &Path) -> Result<(), Error> {
    Err(Error::NotImplemented)
  }
}
