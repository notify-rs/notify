//! Stub Watcher implementation

#![allow(unused_variables)]

use std::sync::mpsc::Sender;
use std::path::Path;
use std::time::Duration;
use super::{RawEvent, DebouncedEvent, Result, Watcher, RecursiveMode};

/// Stub `Watcher` implementation
///
/// Events are never delivered from this watcher.
pub struct NullWatcher;

impl Watcher for NullWatcher {
    fn new_raw(tx: Sender<RawEvent>) -> Result<NullWatcher> {
        Ok(NullWatcher)
    }

    fn new(tx: Sender<DebouncedEvent>, delay: Duration) -> Result<NullWatcher> {
        Ok(NullWatcher)
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        Ok(())
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        Ok(())
    }
}
