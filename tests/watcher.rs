extern crate notify;
extern crate tempdir;
extern crate tempfile;
extern crate time;

mod utils;

use notify::*;
use std::sync::mpsc::{self, channel};
use tempdir::TempDir;
use std::fs;
use std::thread;
use std::time::Duration;

use utils::*;

#[cfg(target_os = "linux")]
#[test]
fn new_inotify() {
    let (tx, _) = channel();
    let w: Result<INotifyWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[cfg(target_os = "macos")]
#[test]
fn new_fsevent() {
    let (tx, _) = channel();
    let w: Result<FsEventWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[test]
fn new_null() {
    let (tx, _) = channel();
    let w: Result<NullWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[test]
fn new_poll() {
    let (tx, _) = channel();
    let w: Result<PollWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[test]
fn new_recommended() {
    let (tx, _) = channel();
    let w: Result<RecommendedWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

// if this test builds, it means RecommendedWatcher is Send.
#[test]
fn test_watcher_send() {
    let (tx, _) = channel();

    let mut watcher: RecommendedWatcher = Watcher::new(tx).unwrap();

    thread::spawn(move || {
        watcher.watch(".", RecursiveMode::Recursive).unwrap();
    }).join().unwrap();
}

// if this test builds, it means RecommendedWatcher is Sync.
#[test]
fn test_watcher_sync() {
    use std::sync::{ Arc, RwLock };

    let (tx, _) = channel();

    let watcher: RecommendedWatcher = Watcher::new(tx).unwrap();
    let watcher = Arc::new(RwLock::new(watcher));

    thread::spawn(move || {
        let mut watcher = watcher.write().unwrap();
        watcher.watch(".", RecursiveMode::Recursive).unwrap();
    }).join().unwrap();
}
