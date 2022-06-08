use std::{
    env,
    path::{Path, PathBuf},
};

use crossbeam_channel::{unbounded, Receiver};
use notify::*;

use utils::*;
mod utils;

const TEMP_DIR: &str = "temp_dir";

#[cfg(target_os = "windows")]
fn recv_events_simple(rx: &Receiver<Result<Event>>) -> Vec<(PathBuf, EventKind, Option<usize>)> {
    recv_events(&rx)
}

#[cfg(target_os = "macos")]
fn recv_events_simple(rx: &Receiver<Result<Event>>) -> Vec<(PathBuf, EventKind, Option<usize>)> {
    let mut events = Vec::new();
    for (path, ev, cookie) in recv_events(&rx) {
        if let EventKind::Create(_) | EventKind::Modify(event::ModifyKind::Data(_)) = ev {
            events.push((
                path,
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                cookie,
            ));
        } else {
            events.push((path, ev, cookie));
        }
    }
    events
}

#[cfg(target_os = "linux")]
fn recv_events_simple(rx: &Receiver<Result<Event>>) -> Vec<(PathBuf, EventKind, Option<usize>)> {
    let mut events = recv_events(rx);
    events.retain(|(_, ref ev, _)| matches!(ev, EventKind::Access(_)));
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

        let (tx, rx) = unbounded();
        let mut watcher: RecommendedWatcher =
            Watcher::new(tx).expect("failed to create recommended watcher");
        watcher
            .watch(Path::new("dir1"), RecursiveMode::Recursive)
            .expect("failed to watch directory");

        sleep_windows(100);

        tdir.create("dir1/file1");

        if cfg!(target_os = "macos") {
            assert_eq!(
                recv_events_simple(&rx),
                vec![
                    (
                        tdir.mkpath("dir1/file1"),
                        EventKind::Create(event::CreateKind::Any),
                        None
                    ), // fsevents always returns canonicalized paths
                ]
            );
        } else {
            assert_eq!(
                recv_events_simple(&rx),
                vec![(
                    tdir.path().join("dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),]
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

        let (tx, rx) = unbounded();
        let mut watcher: RecommendedWatcher =
            Watcher::new(tx).expect("failed to create recommended watcher");
        watcher
            .watch(Path::new("file1"), RecursiveMode::Recursive)
            .expect("failed to watch file");

        sleep_windows(100);

        tdir.write("file1");

        if cfg!(target_os = "macos") {
            assert_eq!(
                recv_events_simple(&rx),
                vec![
                    (
                        tdir.mkpath("file1"),
                        EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                        None
                    ), // fsevents always returns canonicalized paths
                ]
            );
        } else {
            assert_eq!(
                recv_events_simple(&rx),
                vec![(
                    tdir.path().join("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ),]
            );
        }
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
    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1/file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events_simple(&rx),
            vec![
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // fsevents always returns canonicalized paths
            ]
        );
    } else {
        assert_eq!(
            recv_events_simple(&rx),
            vec![(
                watch_path.join("file1"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),]
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
    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events_simple(&rx),
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // fsevents always returns canonicalized paths
            ]
        );
    } else {
        assert_eq!(
            recv_events_simple(&rx),
            vec![(
                watch_path,
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            ),]
        );
    }
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
    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1/file1");

    assert_eq!(
        recv_events_simple(&rx),
        vec![(
            watch_path.join("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
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
    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    assert_eq!(
        recv_events_simple(&rx),
        vec![(
            watch_path,
            EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
            None
        ),]
    );
}
