#![allow(dead_code)]

extern crate notify;
extern crate tempfile;

#[macro_use]
mod utils;

use notify::*;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use utils::*;

const DELAY_MS: u64 = 1000;
const TIMEOUT_MS: u64 = 1000;
const TEMP_DIR: &str = "temp_dir";

fn recv_events_debounced(rx: &mpsc::Receiver<DebouncedEvent>) -> Vec<DebouncedEvent> {
    let start = Instant::now();

    let mut events = Vec::new();

    while start.elapsed() < Duration::from_millis(DELAY_MS + TIMEOUT_MS) {
        match rx.try_recv() {
            Ok(event) => events.push(event),
            Err(mpsc::TryRecvError::Empty) => (),
            Err(e) => panic!("unexpected channel err: {:?}", e),
        }
        thread::sleep(Duration::from_millis(50));
    }
    events
}

#[test]
fn create_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![DebouncedEvent::Create(tdir.mkpath("file1")),]
    );
}

#[test]
fn write_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.write("file1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
            DebouncedEvent::Write(tdir.mkpath("file1")),
        ]
    );
}

#[test]
fn write_long_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    let wait = Duration::from_millis(DELAY_MS / 2);
    tdir.write("file1");
    thread::sleep(wait);
    tdir.write("file1");
    thread::sleep(wait);
    tdir.write("file1");
    thread::sleep(wait);
    tdir.write("file1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
            DebouncedEvent::Write(tdir.mkpath("file1")),
        ]
    );
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
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.chmod("file1");

    if cfg!(target_os = "windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
                DebouncedEvent::Write(tdir.mkpath("file1")),
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![DebouncedEvent::Chmod(tdir.mkpath("file1")),]
        );
    }
}

#[test]
fn delete_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.remove("file1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Remove(tdir.mkpath("file1")),
        ]
    );
}

#[test]
fn rename_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1", "file2");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
        ]
    );
}

#[test]
fn create_write_modify_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");
    tdir.write("file1");
    tdir.chmod("file1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![DebouncedEvent::Create(tdir.mkpath("file1")),]
    );
}

#[test]
fn create_delete_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");
    sleep_macos(10);
    tdir.remove("file1");

    assert_eq!(recv_events_debounced(&rx), vec![]);
}

#[test]
fn delete_create_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.remove("file1");
    sleep_macos(10);
    tdir.create("file1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Write(tdir.mkpath("file1")),
        ]
    );
}

#[test]
fn create_rename_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");
    tdir.rename("file1", "file2");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![DebouncedEvent::Create(tdir.mkpath("file2")),]
    );
}

#[test]
fn create_rename_delete_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

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
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file2"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");
    tdir.rename("file1", "file2");

    if cfg!(target_os = "windows") || cfg!(target_os = "macos") {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("file2")),
                DebouncedEvent::Create(tdir.mkpath("file2")), // even though the file is being overwritten, that can't be detected
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::Create(tdir.mkpath("file2")), // even though the file is being overwritten, that can't be detected
            ]
        );
    }
}

// https://github.com/passcod/notify/issues/99
#[test]
fn create_rename_write_create() {
    // fsevents
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");
    tdir.rename("file1", "file2");
    sleep(10);
    tdir.write("file2");
    tdir.create("file3");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::Create(tdir.mkpath("file2")),
            DebouncedEvent::Create(tdir.mkpath("file3")),
        ]
    );
}

// https://github.com/passcod/notify/issues/100
#[test]
fn create_rename_remove_create() {
    // fsevents
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");

    sleep_macos(35_000);

    tdir.rename("file1", "file2");
    tdir.remove("file2");
    sleep_macos(10);
    tdir.create("file3");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::Create(tdir.mkpath("file1")),
                DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
                DebouncedEvent::NoticeRemove(tdir.mkpath("file2")),
                // DebouncedEvent::Remove(tdir.mkpath("file1")), BUG: There should be a remove event for file1
                DebouncedEvent::Remove(tdir.mkpath("file2")),
                DebouncedEvent::Create(tdir.mkpath("file3")),
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![DebouncedEvent::Create(tdir.mkpath("file3")),]
        );
    }
}

// https://github.com/passcod/notify/issues/101
#[test]
fn move_out_sleep_move_in() {
    // fsevents
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create("watch_dir");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("watch_dir/file1");
    tdir.rename("watch_dir/file1", "file1");
    sleep(DELAY_MS + 10);
    tdir.rename("file1", "watch_dir/file2");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::Create(tdir.mkpath("watch_dir/file1")),
                DebouncedEvent::NoticeRemove(tdir.mkpath("watch_dir/file2")),
                DebouncedEvent::Create(tdir.mkpath("watch_dir/file2")),
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![DebouncedEvent::Create(tdir.mkpath("watch_dir/file2")),]
        );
    }
}

// A stress test that is moving files around trying to trigger possible bugs related to moving files.
// For example, with inotify it's possible that two connected move events are split
// between two mio polls. This doesn't happen often, though.
#[test]
#[ignore]
fn move_repeatedly() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create("watch_dir");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("watch_dir/file1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![DebouncedEvent::Create(tdir.mkpath("watch_dir/file1")),]
    );

    for i in 1..300 {
        let from = format!("watch_dir/file{}", i);
        let to = format!("watch_dir/file{}", i + 1);
        tdir.rename(&from, &to);

        if i % 10 == 0 {
            let from = format!("watch_dir/file{}", i - 9);
            assert_eq!(
                recv_events_debounced(&rx),
                vec![
                    DebouncedEvent::NoticeRemove(tdir.mkpath(&from)),
                    DebouncedEvent::Rename(tdir.mkpath(&from), tdir.mkpath(&to)),
                ]
            );
        }
    }
}

#[test]
fn write_rename_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.write("file1");
    tdir.rename("file1", "file2");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
            DebouncedEvent::Write(tdir.mkpath("file2")),
        ]
    );
}

#[test]
fn rename_write_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.write("file2");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::NoticeWrite(tdir.mkpath("file2")), // TODO not necessary
            DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
            DebouncedEvent::Write(tdir.mkpath("file2")),
        ]
    );
}

// FSEvent just doesn't see those :(
#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn truncate_write_file() {
    use std::io::Write;

    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(tdir.mkpath("file1"))
        .unwrap();
    file.write_all(b"foofoofoo").unwrap();

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    let mut file = std::fs::OpenOptions::new()
        .truncate(true)
        .write(true)
        .open(tdir.mkpath("file1"))
        .unwrap();
    file.write_all(b"barbarbarbar").unwrap();

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
            DebouncedEvent::Write(tdir.mkpath("file1")),
        ]
    );
}

#[test]
fn modify_rename_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.chmod("file1");
    tdir.rename("file1", "file2");

    if cfg!(target_os = "windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
                DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
                DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
                DebouncedEvent::Write(tdir.mkpath("file2")),
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
                DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
                DebouncedEvent::Chmod(tdir.mkpath("file2")),
            ]
        );
    }
}

#[test]
fn rename_modify_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.chmod("file2");

    if cfg!(target_os = "windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
                DebouncedEvent::NoticeWrite(tdir.mkpath("file2")), // TODO unnecessary
                DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
                DebouncedEvent::Write(tdir.mkpath("file2")),
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
                DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file2")),
                DebouncedEvent::Chmod(tdir.mkpath("file2")),
            ]
        );
    }
}

#[test]
fn rename_rename_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.rename("file2", "file3");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Rename(tdir.mkpath("file1"), tdir.mkpath("file3")),
        ]
    );
}

#[test]
fn write_delete_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.write("file1");
    tdir.remove("file1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Remove(tdir.mkpath("file1")),
        ]
    );
}

#[test]
fn create_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("dir1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![DebouncedEvent::Create(tdir.mkpath("dir1")),]
    );
}

// https://github.com/passcod/notify/issues/124
#[test]
fn create_directory_watch_subdirectories() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("dir1");
    tdir.create("dir1/dir2");

    sleep(100);

    tdir.create("dir1/dir2/file1");

    if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::Create(tdir.mkpath("dir1")),
                DebouncedEvent::Create(tdir.mkpath("dir1/dir2/file1")),
            ]
        );
    } else {
        /*  */
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::Create(tdir.mkpath("dir1")),
                DebouncedEvent::Create(tdir.mkpath("dir1/dir2")),
                DebouncedEvent::Create(tdir.mkpath("dir1/dir2/file1")),
            ]
        );
    }
}

#[test]
fn modify_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.chmod("dir1");

    if cfg!(target_os = "windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeWrite(tdir.mkpath("dir1")),
                DebouncedEvent::Write(tdir.mkpath("dir1")),
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![DebouncedEvent::Chmod(tdir.mkpath("dir1")),]
        );
    }
}

#[test]
fn delete_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.remove("dir1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Remove(tdir.mkpath("dir1")),
        ]
    );
}

#[test]
fn rename_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("dir1", "dir2");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
        ]
    );
}

#[test]
fn create_modify_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("dir1");
    tdir.chmod("dir1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![DebouncedEvent::Create(tdir.mkpath("dir1")),]
    );
}

#[test]
fn create_delete_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("dir1");
    sleep_macos(10);
    tdir.remove("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![]);
}

#[test]
fn delete_create_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.remove("dir1");
    sleep_macos(10);
    tdir.create("dir1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Write(tdir.mkpath("dir1")),
        ]
    );
}

#[test]
fn create_rename_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("dir1");
    tdir.rename("dir1", "dir2");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![DebouncedEvent::Create(tdir.mkpath("dir2")),]
    );
}

#[test]
fn create_rename_delete_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("dir1");
    sleep_macos(10);
    tdir.rename("dir1", "dir2");
    sleep_macos(10);
    tdir.remove("dir2");

    assert_eq!(recv_events_debounced(&rx), vec![]);
}

#[test]
#[cfg_attr(target_os = "windows", ignore)]
fn create_rename_overwrite_directory() {
    // overwriting directories doesn't work on windows
    if cfg!(target_os = "windows") {
        panic!("cannot overwrite directory on windows");
    }

    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir2"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("dir1");
    tdir.rename("dir1", "dir2");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("dir2")), // even though the directory is being overwritten, that can't be detected
                DebouncedEvent::Create(tdir.mkpath("dir2")), // even though the directory is being overwritten, that can't be detected
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::Create(tdir.mkpath("dir2")), // even though the directory is being overwritten, that can't be detected
            ]
        );
    }
}

#[test]
fn modify_rename_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.chmod("dir1");
    tdir.chmod("dir1"); // needed by macOS
    tdir.rename("dir1", "dir2");

    if cfg!(target_os = "windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeWrite(tdir.mkpath("dir1")),
                DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
                DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
                DebouncedEvent::Write(tdir.mkpath("dir2")),
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
                DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
                DebouncedEvent::Chmod(tdir.mkpath("dir2")),
            ]
        );
    }
}

#[test]
fn rename_modify_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("dir1", "dir2");
    sleep_macos(10);
    tdir.chmod("dir2");

    let actual = recv_events_debounced(&rx);

    if cfg!(target_os = "windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(
            actual,
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
                DebouncedEvent::NoticeWrite(tdir.mkpath("dir2")), // TODO unnecessary
                DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
                DebouncedEvent::Write(tdir.mkpath("dir2")),
            ]
        );
    } else if cfg!(target_os = "linux") {
        assert_eq_any!(
            actual,
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
                DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
                DebouncedEvent::Chmod(tdir.mkpath("dir2")),
            ],
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
                DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
                DebouncedEvent::Chmod(tdir.mkpath("dir2")),
                DebouncedEvent::Chmod(tdir.mkpath("dir1")), // excessive chmod event
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
                DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir2")),
                DebouncedEvent::Chmod(tdir.mkpath("dir2")),
            ]
        );
    }
}

#[test]
fn rename_rename_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("dir1", "dir2");
    sleep_macos(10);
    tdir.rename("dir2", "dir3");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
            DebouncedEvent::Rename(tdir.mkpath("dir1"), tdir.mkpath("dir3")),
        ]
    );
}

#[test]
fn modify_delete_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.chmod("dir1");
    tdir.chmod("dir1"); // needed by windows
    tdir.remove("dir1");

    if cfg!(target_os = "windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeWrite(tdir.mkpath("dir1")),
                DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
                DebouncedEvent::Remove(tdir.mkpath("dir1")),
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("dir1")),
                DebouncedEvent::Remove(tdir.mkpath("dir1")),
            ]
        );
    }
}

// consistent failures on macOS CI — ignored for now
// https://github.com/passcod/notify/issues/124
#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn move_in_directory_watch_subdirectories() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["watch_dir", "dir1/dir2"]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("dir1", "watch_dir/dir1");

    sleep(100);

    tdir.create("watch_dir/dir1/dir2/file1");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::Create(tdir.mkpath("watch_dir/dir1")),
            DebouncedEvent::Create(tdir.mkpath("watch_dir/dir1/dir2/file1")),
        ]
    );
}

// https://github.com/passcod/notify/issues/129
#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn rename_create_remove_temp_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create("file1");
    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_macos(10);
    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.create("file1");
    tdir.remove("file2");

    assert_eq!(
        recv_events_debounced(&rx),
        vec![
            DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
            DebouncedEvent::Create(tdir.mkpath("file1")),
        ]
    );
}

#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn rename_rename_remove_temp_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create("file1");
    tdir.create("file3");
    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_macos(10);
    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.rename("file3", "file1");
    sleep_macos(10);
    tdir.remove("file2");

    if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
                DebouncedEvent::NoticeRemove(tdir.mkpath("file3")),
                DebouncedEvent::NoticeWrite(tdir.mkpath("file1")),
                DebouncedEvent::Rename(tdir.mkpath("file3"), tdir.mkpath("file1")),
                DebouncedEvent::Write(tdir.mkpath("file1")),
            ]
        );
    } else {
        assert_eq!(
            recv_events_debounced(&rx),
            vec![
                DebouncedEvent::NoticeRemove(tdir.mkpath("file1")),
                DebouncedEvent::NoticeRemove(tdir.mkpath("file3")),
                DebouncedEvent::Rename(tdir.mkpath("file3"), tdir.mkpath("file1")),
            ]
        );
    }
}

#[test]
fn watcher_terminates() {
    let (tx, rx) = mpsc::channel();
    let watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(DELAY_MS))
        .expect("failed to create debounced watcher");
    let thread = thread::spawn(move || {
        for e in rx.into_iter() {
            println!("{:?}", e);
        }
    });
    drop(watcher);
    thread.join().unwrap();
}

#[test]
fn one_file_many_events() {
    let delay = Duration::from_millis(250);
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");
    let dir = tdir.path();

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx, delay).expect("failed to create debounced watcher");

    watcher.watch(&dir, RecursiveMode::Recursive).unwrap();

    let dir = dir.to_path_buf();
    let io_duration = Duration::from_millis(500);
    let now = Instant::now();
    // spam with writes
    let io_thread = thread::spawn(move || {
        use std::fs::File;
        use std::io::prelude::*;
        let mut file = File::create(dir.join("foo.txt")).unwrap();
        while now.elapsed() < io_duration {
            file.write_all(b"Hello, world!").unwrap();
        }
    });
    // wait for 1 event
    let _ = rx.recv().unwrap();
    // should be recieved within the delay since the end of the writes
    let cutoff = io_duration + delay + delay / 10;
    let elapsed = now.elapsed();
    assert!(
        elapsed < cutoff,
        "elapsed: {:?}, cutoff: {:?}",
        elapsed,
        cutoff
    );
    io_thread.join().unwrap();
}

// https://github.com/passcod/notify/issues/205
#[test]
fn delay_zero() {
    let _timeout_test = fail_after("delay_zero", Duration::from_secs(2));
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create("file1");

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx, Duration::from_secs(0)).expect("failed to create debounced watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    let thread = thread::spawn(move || for _ in rx.into_iter() {});

    for _ in 0..100 {
        tdir.rename("file1", "file2");
        tdir.rename("file2", "file1");
    }

    drop(watcher);
    thread.join().unwrap();
}
