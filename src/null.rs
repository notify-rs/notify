//! Stub Watcher implementation

#![allow(unused_variables)]

use super::{RecursiveMode, Result, Watcher};
use std::path::Path;

/// Stub `Watcher` implementation
///
/// Events are never delivered from this watcher.
pub struct NullWatcher;

impl Watcher for NullWatcher {
    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) -> Result<()> {
        Ok(())
    }

    fn new<F: crate::EventFn>(event_fn: F) -> Result<Self> where Self: Sized {
        Ok(NullWatcher)
    }
}
