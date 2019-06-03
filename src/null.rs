//! Stub Watcher implementation

#![allow(unused_variables)]

use super::{Event, RecursiveMode, Result, Watcher};
use crossbeam_channel::Sender;
use std::path::Path;
use std::time::Duration;

/// Stub `Watcher` implementation
///
/// Events are never delivered from this watcher.
pub struct NullWatcher;

impl Watcher for NullWatcher {
    fn new_immediate(tx: Sender<Result<Event>>) -> Result<NullWatcher> {
        Ok(NullWatcher)
    }

    fn new(tx: Sender<Result<Event>>, delay: Duration) -> Result<NullWatcher> {
        Ok(NullWatcher)
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        Ok(())
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        Ok(())
    }
}
