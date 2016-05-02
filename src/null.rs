//! Stub Watcher implementation

#![allow(unused_variables)]

use std::sync::mpsc::Sender;
use std::path::Path;
use super::{Error, Event, Watcher};

/// Stub `Watcher` implementation
///
/// Events are never delivered from this watcher.
pub struct NullWatcher;

impl Watcher for NullWatcher {
    fn new(_tx: Sender<Event>) -> Result<NullWatcher, Error> {
        Ok(NullWatcher)
    }

    fn watch<P: AsRef<Path>>(&mut self, _path: P) -> Result<(), Error> {
        Ok(())
    }

    fn unwatch<P: AsRef<Path>>(&mut self, _path: P) -> Result<(), Error> {
        Ok(())
    }
}
