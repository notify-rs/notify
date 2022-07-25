#![cfg(target_os = "windows")]
#![cfg(feature = "network_tests")]

use std::{env, path::Path};

use crossbeam_channel::unbounded;
use notify::*;
use tempfile::TempDir;

use utils::*;
mod utils;

const NETWORK_PATH: &str = ""; // eg.: \\\\MY-PC\\Users\\MyName

#[test]
fn watch_relative_network_directory() {
    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("dir1");

    env::set_current_dir(tdir.path()).expect("failed to change working directory");

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(Path::new("dir1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1/file1");

    assert_eq!(
        recv_events(&rx),
        vec![(
            tdir.path().join("dir1/file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );
}

#[test]
fn watch_relative_network_file() {
    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("file1");

    env::set_current_dir(tdir.path()).expect("failed to change working directory");

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(Path::new("file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    sleep_windows(100);

    tdir.write("file1");

    assert_eq!(
        recv_events(&rx),
        vec![(
            tdir.path().join("file1"),
            EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
            None
        ),]
    );
}

#[test]
fn watch_absolute_network_directory() {
    if NETWORK_PATH.is_empty() {
        return;
    }

    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("dir1");

    let watch_path = tdir.path().join("dir1");
    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1/file1");

    assert_eq!(
        recv_events(&rx),
        vec![(
            watch_path.join("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );
}

#[test]
fn watch_absolute_network_file() {
    if NETWORK_PATH.is_empty() {
        return;
    }

    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("file1");

    let watch_path = tdir.path().join("file1");
    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    assert_eq!(
        recv_events(&rx),
        vec![(
            watch_path,
            EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
            None
        ),]
    );
}

#[test]
fn watch_canonicalized_network_directory() {
    if NETWORK_PATH.is_empty() {
        return;
    }

    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("dir1");

    let watch_path = tdir
        .path()
        .canonicalize()
        .expect("failed to canonicalize path")
        .join("dir1");
    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1/file1");

    assert_eq!(
        recv_events(&rx),
        vec![(
            watch_path.join("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );
}

#[test]
fn watch_canonicalized_network_file() {
    if NETWORK_PATH.is_empty() {
        return;
    }

    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("file1");

    let watch_path = tdir
        .path()
        .canonicalize()
        .expect("failed to canonicalize path")
        .join("file1");
    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    assert_eq!(
        recv_events(&rx),
        vec![(
            watch_path,
            EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
            None
        ),]
    );
}
