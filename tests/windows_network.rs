#![cfg(target_os = "windows")]
#![cfg(feature = "network_tests")]

use notify::*;
use std::{env, path::Path, sync::mpsc};
use tempfile::TempDir;

use utils::*;
mod utils;

const NETWORK_PATH: &str = ""; // eg.: \\\\MY-PC\\Users\\MyName

#[test]
fn watch_relative_network_directory() {
    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("dir1");

    env::set_current_dir(tdir.path()).expect("failed to change working directory");

    let (tx, _) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(Path::new("dir1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    watcher
        .unwatch(Path::new("dir1"))
        .expect("failed to unwatch directory");

    if cfg!(not(target_os = "windows")) {
        match watcher.unwatch(Path::new("dir1")) {
            Err(Error {
                kind: ErrorKind::WatchNotFound,
                ..
            }) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}

#[test]
fn watch_relative_network_file() {
    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("file1");

    env::set_current_dir(tdir.path()).expect("failed to change working directory");

    let (tx, _) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(Path::new("file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    watcher
        .unwatch(Path::new("file1"))
        .expect("failed to unwatch file");

    if cfg!(not(target_os = "windows")) {
        match watcher.unwatch(Path::new("file1")) {
            Err(Error {
                kind: ErrorKind::WatchNotFound,
                ..
            }) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}

#[test]
fn watch_absolute_network_directory() {
    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("dir1");

    let (tx, _) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("dir1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    watcher
        .unwatch(&tdir.mkpath("dir1"))
        .expect("failed to unwatch directory");

    if cfg!(not(target_os = "windows")) {
        match watcher.unwatch(&tdir.mkpath("dir1")) {
            Err(Error {
                kind: ErrorKind::WatchNotFound,
                ..
            }) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}

#[test]
fn watch_absolute_network_file() {
    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("file1");

    env::set_current_dir(tdir.path()).expect("failed to change working directory");

    let (tx, _) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    watcher
        .unwatch(&tdir.mkpath("file1"))
        .expect("failed to unwatch file");

    if cfg!(not(target_os = "windows")) {
        match watcher.unwatch(&tdir.mkpath("file1")) {
            Err(Error {
                kind: ErrorKind::WatchNotFound,
                ..
            }) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}
