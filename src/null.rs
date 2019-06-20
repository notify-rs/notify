//! Stub Watcher implementation

#![allow(unused_variables)]

use super::{RawEvent, RecursiveMode, Result, Watcher};
use crossbeam_channel::Sender;
use std::path::Path;

/// Stub `Watcher` implementation
///
/// Events are never delivered from this watcher.
pub struct NullWatcher;

impl Watcher for NullWatcher {
    fn new_immediate(tx: Sender<RawEvent>) -> Result<NullWatcher> {
        Ok(NullWatcher)
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        Ok(())
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        Ok(())
    }
}
