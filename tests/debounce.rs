#![allow(dead_code)]

extern crate notify;
extern crate tempdir;
extern crate time;

#[macro_use]
mod utils;

use notify::*;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tempdir::TempDir;
use utils::*;

const DELAY_S: u64 = 1;
const TIMEOUT_S: f64 = 1.0;

fn recv_events_debounced(rx: &mpsc::Receiver<DebouncedEvent>) -> Vec<DebouncedEvent> {
    let deadline = time::precise_time_s() + DELAY_S as f64 + TIMEOUT_S;

    let mut events = Vec::new();

    while time::precise_time_s() < deadline {
        match rx.try_recv() {
            Ok(event) => events.push(event),
            Err(mpsc::TryRecvError::Empty) => (),
            Err(e) => panic!("unexpected channel err: {:?}", e)
        }
        thread::sleep(Duration::from_millis(50));
    }
    events
}

#[test]
fn create_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::Create(tdir.mkpath("file1")),
    ]);
}

#[test]
fn write_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.write("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
        DebouncedEvent::Write(tdir.mkpath("file1")),
    ]);
}

#[test]
fn write_long_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    let wait = Duration::from_millis(DELAY_S * 500);
    tdir.write("file1");
    thread::sleep(wait);
    tdir.write("file1");
    thread::sleep(wait);
    tdir.write("file1");
    thread::sleep(wait);
    tdir.write("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
        DebouncedEvent::Write(tdir.mkpath("file1")),
    ]);
}

// Linux:
//
// thread 'write_long_file' panicked at 'assertion failed: `(left == right)`
// (left: `[
//   NoticeWrite("/tmp/temp_dir.fZov9D5M7lQ6/file1"),
//   Write("/tmp/temp_dir.fZov9D5M7lQ6/file1"),
//   NoticeWrite("/tmp/temp_dir.fZov9D5M7lQ6/file1"),
//   Write("/tmp/temp_dir.fZov9D5M7lQ6/file1")
// ]`,
// right: `[
//   NoticeWrite("/tmp/temp_dir.fZov9D5M7lQ6/file1"),
//   Write("/tmp/temp_dir.fZov9D5M7lQ6/file1")
// ]`)',
// tests/debounce.rs:100

#[test]
fn modify_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("file1");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
            DebouncedEvent::Write(tdir.mkpath("file1")),
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::Chmod(tdir.mkpath("file1")),
        ]);
    }
}

#[test]
fn delete_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.remove("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
        DebouncedEvent::Remove(tdir.mkpath("file1")),
    ]);
}

#[test]
fn rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("file1", "file2");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
        DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
    ]);
}

#[test]
fn create_write_modify_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    tdir.write("file1");
    tdir.chmod("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::Create(tdir.mkpath("file1")),
    ]);
}

#[test]
fn create_delete_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    sleep_macos(10);
    tdir.remove("file1");

    assert_eq!(recv_events_debounced(&rx), vec![]);
}

#[test]
fn delete_create_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.remove("file1");
    sleep_macos(10);
    tdir.create("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
        DebouncedEvent::Write(tdir.mkpath("file1")),
    ]);
}

#[test]
fn create_rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    tdir.rename("file1", "file2");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::Create(tdir.mkpath("file2")),
    ]);
}

#[test]
fn create_rename_delete_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    sleep_macos(10);
    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.remove("file2");

    assert_eq!(recv_events_debounced(&rx), vec![]);
}

// ---- create_rename_delete_file stdout ----

// Mac OS
//
// thread 'create_rename_delete_file' panicked at 'assertion failed: `(left == right)`
// (left: `[
//   NoticeRemove("/private/var/folders/gw/_2jq29095y7b__wtby9dg_5h0000gn/T/temp_dir.MJM4fvovN8qg/file2"),
//   Remove("/private/var/folders/gw/_2jq29095y7b__wtby9dg_5h0000gn/T/temp_dir.MJM4fvovN8qg/file2")
// ]`,
// right: `[]`)',
// tests/debounce.rs:273

#[test]
fn create_rename_overwrite_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file2",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    tdir.rename("file1", "file2");

    if cfg!(target_os="windows") {
        // Windows interprets a move that overwrites a file as a delete of the source file and a write to the file that is being overwritten
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("file2")),
            DebouncedEvent::Write(tdir.mkpath("file2")),
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file2")),
            DebouncedEvent::Create(tdir.mkpath("file2")), // even though the file is being overwritten, that can't be detected
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::Create(tdir.mkpath("file2")), // even though the file is being overwritten, that can't be detected
        ]);
    }
}

// https://github.com/passcod/notify/issues/99
#[test]
fn create_rename_write_create() { // fsevents
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    tdir.rename("file1", "file2");
    sleep(10);
    tdir.write("file2");
    tdir.create("file3");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::Create(tdir.mkpath("file2")),
        DebouncedEvent::Create(tdir.mkpath("file3")),
    ]);
}

// https://github.com/passcod/notify/issues/100
#[test]
fn create_rename_remove_create() { // fsevents
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");

    sleep_macos(35_000);

    tdir.rename("file1", "file2");
    tdir.remove("file2");
    sleep_macos(10);
    tdir.create("file3");

    if cfg!(target_os="macos") {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::Create(tdir.mkpath("file1")),
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::NoticeRemove(tdir.mkpath("file2")),
            // DebouncedEvent::Remove(tdir.mkpath("file1")), BUG: There should be a remove event for file1
            DebouncedEvent::Remove(tdir.mkpath("file2")),
            DebouncedEvent::Create(tdir.mkpath("file3")),
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::Create(tdir.mkpath("file3")),
        ]);
    }
}

// https://github.com/passcod/notify/issues/101
#[test]
fn move_out_sleep_move_in() { // fsevents
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create("watch_dir");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("watch_dir/file1");
    tdir.rename("watch_dir/file1", "file1");
    sleep(DELAY_S * 1000 + 10);
    tdir.rename("file1", "watch_dir/file2");

    if cfg!(target_os="macos") {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::Create(tdir.mkpath("watch_dir/file1")),
            DebouncedEvent::NoticeRemove(tdir.mkpath("watch_dir/file2")),
            DebouncedEvent::Create(tdir.mkpath("watch_dir/file2")),
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::Create(tdir.mkpath("watch_dir/file2")),
        ]);
    }
}

#[test]
fn write_rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.write("file1");
    tdir.rename("file1", "file2");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
        DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
        DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
        DebouncedEvent::Write(tdir.mkpath("file2")),
    ]);
}

#[test]
fn rename_write_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.write("file2");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
        DebouncedEvent::NoticeWrite(tdir.mkpath("file2")), // TODO not necessary
        DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
        DebouncedEvent::Write(tdir.mkpath("file2")),
    ]);
}

#[test]
fn modify_rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("file1");
    tdir.rename("file1", "file2");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
            DebouncedEvent::Write(tdir.mkpath("file2")),
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
            DebouncedEvent::Chmod(tdir.mkpath("file2")),
        ]);
    }
}

#[test]
fn rename_modify_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.chmod("file2");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::NoticeWrite(tdir.mkpath("file2")), // TODO unnecessary
            DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
            DebouncedEvent::Write(tdir.mkpath("file2")),
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
            DebouncedEvent::Chmod(tdir.mkpath("file2")),
        ]);
    }
}

#[test]
fn rename_rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.rename("file2", "file3");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
        DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file3")),
    ]);
}

#[test]
fn write_delete_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.write("file1");
    tdir.remove("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
        DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
        DebouncedEvent::Remove(tdir.mkpath("file1")),
    ]);
}

#[test]
fn create_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::Create(tdir.mkpath("dir1")),
    ]);
}

#[test]
fn modify_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("dir1");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("dir1")),
            DebouncedEvent::Write(tdir.mkpath("dir1")),
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::Chmod(tdir.mkpath("dir1")),
        ]);
    }
}

#[test]
fn delete_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.remove("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
        DebouncedEvent::Remove(tdir.mkpath("dir1")),
    ]);
}

#[test]
fn rename_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("dir1", "dir2");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
        DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
    ]);
}

#[test]
fn create_modify_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");
    tdir.chmod("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::Create(tdir.mkpath("dir1")),
    ]);
}

#[test]
fn create_delete_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");
    sleep_macos(10);
    tdir.remove("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![]);
}

#[test]
fn delete_create_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.remove("dir1");
    sleep_macos(10);
    tdir.create("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
        DebouncedEvent::Write(tdir.mkpath("dir1")),
    ]);
}

#[test]
fn create_rename_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");
    tdir.rename("dir1", "dir2");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::Create(tdir.mkpath("dir2")),
    ]);
}

#[test]
fn create_rename_delete_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");
    sleep_macos(10);
    tdir.rename("dir1", "dir2");
    sleep_macos(10);
    tdir.remove("dir2");

    assert_eq!(recv_events_debounced(&rx), vec![]);
}

#[test]
#[cfg(not(target_os="windows"))]
fn create_rename_overwrite_directory() {
    // overwriting directories doesn't work on windows
    if cfg!(target_os="windows") {
        panic!("cannot overwrite directory on windows");
    }

    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir2",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");
    tdir.rename("dir1", "dir2");

    if cfg!(target_os="macos") {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir2")), // even though the directory is being overwritten, that can't be detected
            DebouncedEvent::Create(tdir.mkpath("dir2")), // even though the directory is being overwritten, that can't be detected
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::Create(tdir.mkpath("dir2")), // even though the directory is being overwritten, that can't be detected
        ]);
    }
}

#[test]
fn modify_rename_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("dir1");
    tdir.chmod("dir1"); // needed by os x
    tdir.rename("dir1", "dir2");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("dir1")),
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
            DebouncedEvent::Write(tdir.mkpath("dir2")),
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
            DebouncedEvent::Chmod(tdir.mkpath("dir2")),
        ]);
    }
}

#[test]
fn rename_modify_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("dir1", "dir2");
    sleep_macos(10);
    tdir.chmod("dir2");

    let actual = recv_events_debounced(&rx);

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(actual, vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::NoticeWrite(tdir.mkpath("dir2")), // TODO unnecessary
            DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
            DebouncedEvent::Write(tdir.mkpath("dir2")),
        ]);
    } else if cfg!(target_os="linux") {
        assert_eq_any!(actual, vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
            DebouncedEvent::Chmod(tdir.mkpath("dir2")),
        ], vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
            DebouncedEvent::Chmod(tdir.mkpath("dir2")),
            DebouncedEvent::Chmod(tdir.mkpath("dir1")), // excessive chmod event
        ]);
    } else {
        assert_eq!(actual, vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
            DebouncedEvent::Chmod(tdir.mkpath("dir2")),
        ]);
    }
}

#[test]
fn rename_rename_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("dir1", "dir2");
    sleep_macos(10);
    tdir.rename("dir2", "dir3");

    assert_eq!(recv_events_debounced(&rx), vec![
        DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
        DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir3")),
    ]);
}

#[test]
fn modify_delete_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("dir1");
    tdir.chmod("dir1"); // needed by windows
    tdir.remove("dir1");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("dir1")),
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Remove(tdir.mkpath("dir1")),
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Remove(tdir.mkpath("dir1")),
        ]);
    }
}
