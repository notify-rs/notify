//! Stub Watcher implementation

#![allow(unused_variables)]

use super::{Event, RawEvent, RecursiveMode, Result, Watcher};
use crossbeam_channel::Sender;
use std::path::Path;
use std::time::Duration;

/// Stub `Watcher` implementation
///
/// Events are never delivered from this watcher.
pub struct NullWatcher;

impl Watcher for NullWatcher {
    fn new_immediate(tx: Sender<RawEvent>) -> Result<NullWatcher> {
        Ok(NullWatcher)
    }

    fn new(tx: Sender<Event>, delay: Duration) -> Result<NullWatcher> {
        Ok(NullWatcher)
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        Ok(())
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        Ok(())
    }
}
