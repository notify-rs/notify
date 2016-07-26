#![allow(dead_code)]

extern crate notify;
extern crate tempdir;
extern crate time;

mod utils;

use notify::*;
use utils::*;

use tempdir::TempDir;

use std::sync::mpsc;
use std::time::Duration;
use std::thread;

const DELAY_S: u64 = 1;
const TIMEOUT_S: f64 = 1.0;

fn recv_events_debounced(rx: &mpsc::Receiver<debounce::Event>) -> Vec<debounce::Event> {
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::Create{path: tdir.mkpath("file1")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.write("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeWrite{path: tdir.mkpath("file1")},
        debounce::Event::Write{path: tdir.mkpath("file1")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
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
        debounce::Event::NoticeWrite{path: tdir.mkpath("file1")},
        debounce::Event::Write{path: tdir.mkpath("file1")},
    ]);
}

#[test]
fn modify_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("file1");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeWrite{path: tdir.mkpath("file1")},
            debounce::Event::Write{path: tdir.mkpath("file1")},
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::Chmod{path: tdir.mkpath("file1")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.remove("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
        debounce::Event::Remove{path: tdir.mkpath("file1")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("file1", "file2");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
        debounce::Event::Rename{from: tdir.mkpath("file1"), to: tdir.mkpath("file2")},
    ]);
}

#[test]
fn create_write_modify_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    tdir.write("file1");
    tdir.chmod("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::Create{path: tdir.mkpath("file1")},
    ]);
}

#[test]
fn create_delete_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.remove("file1");
    sleep_macos(10);
    tdir.create("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
        debounce::Event::Write{path: tdir.mkpath("file1")},
    ]);
}

#[test]
fn create_rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    tdir.rename("file1", "file2");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::Create{path: tdir.mkpath("file2")},
    ]);
}

#[test]
fn create_rename_delete_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    sleep_macos(10);
    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.remove("file2");

    assert_eq!(recv_events_debounced(&rx), vec![]);
}

#[test]
fn create_rename_overwrite_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file2",
    ]);

    sleep_macos(35_000);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("file1");
    tdir.rename("file1", "file2");

    if cfg!(target_os="windows") {
        // Windows interprets a move that overwrites a file as a delete of the source file and a write to the file that is being overwritten
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeWrite{path: tdir.mkpath("file2")},
            debounce::Event::Write{path: tdir.mkpath("file2")},
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeRemove{path: tdir.mkpath("file2")},
            debounce::Event::Create{path: tdir.mkpath("file2")}, // even though the file is being overwritten, that can't be detected
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::Create{path: tdir.mkpath("file2")}, // even though the file is being overwritten, that can't be detected
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.write("file1");
    tdir.rename("file1", "file2");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeWrite{path: tdir.mkpath("file1")},
        debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
        debounce::Event::Rename{from: tdir.mkpath("file1"), to: tdir.mkpath("file2")},
        debounce::Event::Write{path: tdir.mkpath("file2")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.write("file2");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
        debounce::Event::NoticeWrite{path: tdir.mkpath("file2")}, // TODO not necessary
        debounce::Event::Rename{from: tdir.mkpath("file1"), to: tdir.mkpath("file2")},
        debounce::Event::Write{path: tdir.mkpath("file2")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("file1");
    tdir.rename("file1", "file2");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeWrite{path: tdir.mkpath("file1")},
            debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
            debounce::Event::Rename{from: tdir.mkpath("file1"), to: tdir.mkpath("file2")},
            debounce::Event::Write{path: tdir.mkpath("file2")},
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
            debounce::Event::Rename{from: tdir.mkpath("file1"), to: tdir.mkpath("file2")},
            debounce::Event::Chmod{path: tdir.mkpath("file2")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.chmod("file2");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
            debounce::Event::NoticeWrite{path: tdir.mkpath("file2")}, // TODO unnecessary
            debounce::Event::Rename{from: tdir.mkpath("file1"), to: tdir.mkpath("file2")},
            debounce::Event::Write{path: tdir.mkpath("file2")},
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
            debounce::Event::Rename{from: tdir.mkpath("file1"), to: tdir.mkpath("file2")},
            debounce::Event::Chmod{path: tdir.mkpath("file2")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("file1", "file2");
    sleep_macos(10);
    tdir.rename("file2", "file3");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
        debounce::Event::Rename{from: tdir.mkpath("file1"), to: tdir.mkpath("file3")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.write("file1");
    tdir.remove("file1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeWrite{path: tdir.mkpath("file1")},
        debounce::Event::NoticeRemove{path: tdir.mkpath("file1")},
        debounce::Event::Remove{path: tdir.mkpath("file1")},
    ]);
}

#[test]
fn create_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::Create{path: tdir.mkpath("dir1")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("dir1");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeWrite{path: tdir.mkpath("dir1")},
            debounce::Event::Write{path: tdir.mkpath("dir1")},
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::Chmod{path: tdir.mkpath("dir1")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.remove("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
        debounce::Event::Remove{path: tdir.mkpath("dir1")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("dir1", "dir2");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
        debounce::Event::Rename{from: tdir.mkpath("dir1"), to: tdir.mkpath("dir2")},
    ]);
}

#[test]
fn create_modify_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");
    tdir.chmod("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::Create{path: tdir.mkpath("dir1")},
    ]);
}

#[test]
fn create_delete_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.remove("dir1");
    sleep_macos(10);
    tdir.create("dir1");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
        debounce::Event::Write{path: tdir.mkpath("dir1")},
    ]);
}

#[test]
fn create_rename_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");
    tdir.rename("dir1", "dir2");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::Create{path: tdir.mkpath("dir2")},
    ]);
}

#[test]
fn create_rename_delete_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");
    sleep_macos(10);
    tdir.rename("dir1", "dir2");
    sleep_macos(10);
    tdir.remove("dir2");

    assert_eq!(recv_events_debounced(&rx), vec![]);
}

#[test]
#[ignore]
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.create("dir1");
    tdir.rename("dir1", "dir2");

    if cfg!(target_os="macos") {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeRemove{path: tdir.mkpath("dir2")}, // even though the directory is being overwritten, that can't be detected
            debounce::Event::Create{path: tdir.mkpath("dir2")}, // even though the directory is being overwritten, that can't be detected
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::Create{path: tdir.mkpath("dir2")}, // even though the directory is being overwritten, that can't be detected
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("dir1");
    tdir.chmod("dir1"); // needed by os x
    tdir.rename("dir1", "dir2");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeWrite{path: tdir.mkpath("dir1")},
            debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
            debounce::Event::Rename{from: tdir.mkpath("dir1"), to: tdir.mkpath("dir2")},
            debounce::Event::Write{path: tdir.mkpath("dir2")},
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
            debounce::Event::Rename{from: tdir.mkpath("dir1"), to: tdir.mkpath("dir2")},
            debounce::Event::Chmod{path: tdir.mkpath("dir2")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("dir1", "dir2");
    sleep_macos(10);
    tdir.chmod("dir2");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
            debounce::Event::NoticeWrite{path: tdir.mkpath("dir2")}, // TODO unnecessary
            debounce::Event::Rename{from: tdir.mkpath("dir1"), to: tdir.mkpath("dir2")},
            debounce::Event::Write{path: tdir.mkpath("dir2")},
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
            debounce::Event::Rename{from: tdir.mkpath("dir1"), to: tdir.mkpath("dir2")},
            debounce::Event::Chmod{path: tdir.mkpath("dir2")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.rename("dir1", "dir2");
    sleep_macos(10);
    tdir.rename("dir2", "dir3");

    assert_eq!(recv_events_debounced(&rx), vec![
        debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
        debounce::Event::Rename{from: tdir.mkpath("dir1"), to: tdir.mkpath("dir3")},
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
    let mut watcher: RecommendedWatcher = Watcher::debounced(tx, Duration::from_secs(DELAY_S)).expect("failed to create debounced watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    tdir.chmod("dir1");
    tdir.chmod("dir1"); // needed by windows
    tdir.remove("dir1");

    if cfg!(target_os="windows") {
        // windows cannot distinguish between chmod and write
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeWrite{path: tdir.mkpath("dir1")},
            debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
            debounce::Event::Remove{path: tdir.mkpath("dir1")},
        ]);
    } else {
        assert_eq!(recv_events_debounced(&rx), vec![
            debounce::Event::NoticeRemove{path: tdir.mkpath("dir1")},
            debounce::Event::Remove{path: tdir.mkpath("dir1")},
        ]);
    }
}
