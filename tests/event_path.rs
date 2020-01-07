extern crate notify;
extern crate tempfile;

mod utils;

use notify::*;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use tempfile::TempDir;

use utils::*;

const NETWORK_PATH: &str = ""; // eg.: \\\\MY-PC\\Users\\MyName
const TEMP_DIR: &str = "temp_dir";

#[cfg(target_os = "windows")]
fn recv_events_simple(rx: &Receiver<RawEvent>) -> Vec<(PathBuf, Op, Option<u32>)> {
    recv_events(&rx)
}

#[cfg(target_os = "macos")]
fn recv_events_simple(rx: &Receiver<RawEvent>) -> Vec<(PathBuf, Op, Option<u32>)> {
    let mut events = Vec::new();
    for (path, op, cookie) in inflate_events(recv_events(&rx)) {
        if op == (op::Op::CREATE | op::Op::WRITE) {
            events.push((path, op::Op::WRITE, cookie));
        } else {
            events.push((path, op, cookie));
        }
    }
    events
}

#[cfg(target_os = "linux")]
fn recv_events_simple(rx: &Receiver<RawEvent>) -> Vec<(PathBuf, Op, Option<u32>)> {
    let mut events = recv_events(rx);
    events.retain(|&(_, op, _)| op != op::Op::CLOSE_WRITE);
    events
}

#[test]
fn watch_relative() {
    // both of the following tests set the same environment variable, so they must not run in parallel
    {
        // watch_relative_directory
        let tdir = tempfile::Builder::new()
            .prefix(TEMP_DIR)
            .tempdir()
            .expect("failed to create temporary directory");
        tdir.create("dir1");

        sleep_macos(10);

        env::set_current_dir(tdir.path()).expect("failed to change working directory");

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher =
            Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher
            .watch("dir1", RecursiveMode::Recursive)
            .expect("failed to watch directory");

        sleep_windows(100);

        tdir.create("dir1/file1");

        if cfg!(target_os = "macos") {
            assert_eq!(
                recv_events_simple(&rx),
                vec![
                    (tdir.mkpath("dir1/file1"), op::Op::CREATE, None), // fsevents always returns canonicalized paths
                ]
            );
        } else {
            assert_eq!(
                recv_events_simple(&rx),
                vec![(tdir.path().join("dir1/file1"), op::Op::CREATE, None),]
            );
        }
    }
    {
        // watch_relative_file
        let tdir = tempfile::Builder::new()
            .prefix(TEMP_DIR)
            .tempdir()
            .expect("failed to create temporary directory");
        tdir.create("file1");

        env::set_current_dir(tdir.path()).expect("failed to change working directory");

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher =
            Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher
            .watch("file1", RecursiveMode::Recursive)
            .expect("failed to watch file");

        sleep_windows(100);

        tdir.write("file1");

        if cfg!(target_os = "macos") {
            assert_eq!(
                recv_events_simple(&rx),
                vec![
                    (tdir.mkpath("file1"), op::Op::WRITE, None), // fsevents always returns canonicalized paths
                ]
            );
        } else {
            assert_eq!(
                recv_events_simple(&rx),
                vec![(tdir.path().join("file1"), op::Op::WRITE, None),]
            );
        }
    }
    if cfg!(target_os = "windows") && !NETWORK_PATH.is_empty() {
        // watch_relative_network_directory
        let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
        tdir.create("dir1");

        env::set_current_dir(tdir.path()).expect("failed to change working directory");

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher =
            Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher
            .watch("dir1", RecursiveMode::Recursive)
            .expect("failed to watch directory");

        sleep_windows(100);

        tdir.create("dir1/file1");

        assert_eq!(
            recv_events_simple(&rx),
            vec![(tdir.path().join("dir1/file1"), op::Op::CREATE, None),]
        );
    }
    if cfg!(target_os = "windows") && !NETWORK_PATH.is_empty() {
        // watch_relative_network_file
        let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
        tdir.create("file1");

        env::set_current_dir(tdir.path()).expect("failed to change working directory");

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher =
            Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher
            .watch("file1", RecursiveMode::Recursive)
            .expect("failed to watch file");

        sleep_windows(100);

        tdir.write("file1");

        assert_eq!(
            recv_events_simple(&rx),
            vec![(tdir.path().join("file1"), op::Op::WRITE, None),]
        );
    }
}

#[test]
fn watch_absolute_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");
    tdir.create("dir1");

    sleep_macos(10);

    let watch_path = tdir.path().join("dir1");
    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1/file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events_simple(&rx),
            vec![
                (tdir.mkpath("dir1/file1"), op::Op::CREATE, None), // fsevents always returns canonicalized paths
            ]
        );
    } else {
        assert_eq!(
            recv_events_simple(&rx),
            vec![(watch_path.join("file1"), op::Op::CREATE, None),]
        );
    }
}

#[test]
fn watch_absolute_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");
    tdir.create("file1");

    let watch_path = tdir.path().join("file1");
    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events_simple(&rx),
            vec![
                (tdir.mkpath("file1"), op::Op::WRITE, None), // fsevents always returns canonicalized paths
            ]
        );
    } else {
        assert_eq!(
            recv_events_simple(&rx),
            vec![(watch_path, op::Op::WRITE, None),]
        );
    }
}

#[test]
#[cfg(target_os = "windows")]
fn watch_absolute_network_directory() {
    if NETWORK_PATH.is_empty() {
        return;
    }

    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("dir1");

    let watch_path = tdir.path().join("dir1");
    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1/file1");

    assert_eq!(
        recv_events_simple(&rx),
        vec![(watch_path.join("file1"), op::Op::CREATE, None),]
    );
}

#[test]
#[cfg(target_os = "windows")]
fn watch_absolute_network_file() {
    if NETWORK_PATH.is_empty() {
        return;
    }

    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("file1");

    let watch_path = tdir.path().join("file1");
    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    assert_eq!(
        recv_events_simple(&rx),
        vec![(watch_path, op::Op::WRITE, None),]
    );
}

#[test]
fn watch_canonicalized_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");
    tdir.create("dir1");

    sleep_macos(10);

    let watch_path = tdir
        .path()
        .canonicalize()
        .expect("failed to canonicalize path")
        .join("dir1");
    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1/file1");

    assert_eq!(
        recv_events_simple(&rx),
        vec![(watch_path.join("file1"), op::Op::CREATE, None),]
    );
}

#[test]
fn watch_canonicalized_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");
    tdir.create("file1");

    let watch_path = tdir
        .path()
        .canonicalize()
        .expect("failed to canonicalize path")
        .join("file1");
    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    assert_eq!(
        recv_events_simple(&rx),
        vec![(watch_path, op::Op::WRITE, None),]
    );
}

#[test]
#[cfg(target_os = "windows")]
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
    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1/file1");

    assert_eq!(
        recv_events_simple(&rx),
        vec![(watch_path.join("file1"), op::Op::CREATE, None),]
    );
}

#[test]
#[cfg(target_os = "windows")]
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
    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    assert_eq!(
        recv_events_simple(&rx),
        vec![(watch_path, op::Op::WRITE, None),]
    );
}
