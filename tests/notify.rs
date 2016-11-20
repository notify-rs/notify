extern crate notify;
extern crate tempdir;
extern crate time;

mod utils;

use notify::*;
use std::sync::mpsc;
use std::path::PathBuf;
use tempdir::TempDir;

use utils::*;

#[test]
fn test_inflate_events() {
    assert_eq!(inflate_events(vec![
        (PathBuf::from("file1"), op::CREATE, None),
        (PathBuf::from("file1"), op::WRITE, None),
    ]), vec![
        (PathBuf::from("file1"), op::CREATE | op::WRITE, None),
    ]);

    assert_eq!(inflate_events(vec![
        (PathBuf::from("file1"), op::RENAME, Some(1)),
        (PathBuf::from("file1"), op::RENAME, Some(2)),
    ]), vec![
        (PathBuf::from("file1"), op::RENAME, Some(1)),
        (PathBuf::from("file1"), op::RENAME, Some(2)),
    ]);
}

#[test]
fn create_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    // OSX FsEvent needs some time to discard old events from its log.
    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("file1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
        ]);
    } else if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::CREATE, None)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
            (tdir.mkpath("file1"), op::CLOSE_WRITE, None)
        ]);
    }
}

#[test]
fn write_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1"
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CREATE | op::WRITE, None), // excessive create event
        ]);
    } else if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::WRITE, None)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::WRITE, None),
            (tdir.mkpath("file1"), op::CLOSE_WRITE, None)
        ]);
    }
}

#[test]
#[cfg(not(any(target_os="windows", target_os="macos")))]
fn modify_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1"
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.chmod("file1");

    sleep_macos(10);

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::WRITE, None),
        ]);
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CHMOD | op::CREATE, None),
        ]);
        panic!("macos cannot distinguish between chmod and create");
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::CHMOD, None),
        ]);
    }
}

#[test]
fn delete_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1"
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.remove("file1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CREATE | op::REMOVE, None), // excessive create event
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::REMOVE, None),
        ]);
    }
}

#[test]
fn rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1a"
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("file1a", "file1b");

    if cfg!(target_os="macos") {
        let actual = inflate_events(recv_events(&rx));
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, Some(cookies[0])), // excessive create event
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0]))
        ]);
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0]))
        ]);
    }
}

#[test]
#[cfg(not(target_os="macos"))]
fn move_out_create_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "watch_dir/file1"
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("watch_dir/file1", "file1b");
    tdir.create("watch_dir/file1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("watch_dir/file1"), op::CREATE | op::RENAME, None), // fsevent interprets a move_out as a rename event
        ]);
        panic!("move event should be a remove event; fsevent conflates rename and create events");
    } else if cfg!(target_os="linux") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("watch_dir/file1"), op::REMOVE, None),
            (tdir.mkpath("watch_dir/file1"), op::CREATE, None),
            (tdir.mkpath("watch_dir/file1"), op::CLOSE_WRITE, None),
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("watch_dir/file1"), op::REMOVE, None),
            (tdir.mkpath("watch_dir/file1"), op::CREATE, None),
        ]);
    }
}

#[test]
#[cfg(not(any(target_os="windows", target_os="macos")))]
fn create_write_modify_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("file1");
    tdir.write("file1");
    tdir.chmod("file1");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
            (tdir.mkpath("file1"), op::WRITE, None),
            (tdir.mkpath("file1"), op::WRITE, None),
        ]);
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CHMOD | op::CREATE | op::WRITE, None),
        ]);
        panic!("macos cannot distinguish between chmod and create");
    } else if cfg!(target_os="linux") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
            (tdir.mkpath("file1"), op::CLOSE_WRITE, None),
            (tdir.mkpath("file1"), op::WRITE, None),
            (tdir.mkpath("file1"), op::CLOSE_WRITE, None),
            (tdir.mkpath("file1"), op::CHMOD, None),
        ]);
    }
}

#[test]
fn create_rename_overwrite_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1b",
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("file1a");
    tdir.rename("file1a", "file1b");

    let actual = recv_events(&rx);

    if cfg!(target_os="windows") {
        // Windows interprets a move that overwrites a file as a delete of the source file and a write to the file that is being overwritten
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::CREATE, None),
            (tdir.mkpath("file1a"), op::REMOVE, None),
            (tdir.mkpath("file1b"), op::WRITE, None)
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(actual), vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, None),
            (tdir.mkpath("file1b"), op::CREATE | op::RENAME, None)
        ]);
    } else {
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::CREATE, None),
            (tdir.mkpath("file1a"), op::CLOSE_WRITE, None),
            (tdir.mkpath("file1a"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0]))
        ]);
    }
}

#[test]
fn rename_rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1a",
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("file1a", "file1b");
    tdir.rename("file1b", "file1c");

    if cfg!(target_os="macos") {
        let actual = inflate_events(recv_events(&rx));
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, None),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1c"), op::RENAME, Some(cookies[0]))
        ]);
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[1])),
            (tdir.mkpath("file1c"), op::RENAME, Some(cookies[1]))
        ]);
    }
}

#[test]
fn create_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1");

    let actual = if cfg!(target_os="macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    assert_eq!(actual, vec![
        (tdir.mkpath("dir1"), op::CREATE, None),
    ]);
}

#[test]
#[cfg(not(any(target_os="windows", target_os="macos")))]
fn modify_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.chmod("dir1");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1"), op::WRITE, None),
        ]);
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("dir1"), op::CHMOD | op::CREATE, None),
        ]);
        panic!("macos cannot distinguish between chmod and create");
    } else {
        // TODO: emit chmod event only once
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1"), op::CHMOD, None),
            (tdir.mkpath("dir1"), op::CHMOD, None),
        ]);
    }
}

#[test]
fn delete_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.remove("dir1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("dir1"), op::CREATE | op::REMOVE, None), // excessive create event
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1"), op::REMOVE, None),
        ]);
    }
}

#[test]
fn rename_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1a",
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1a", "dir1b");

    if cfg!(target_os="macos") {
        let actual = inflate_events(recv_events(&rx));
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("dir1a"), op::CREATE | op::RENAME, Some(cookies[0])), // excessive create event
            (tdir.mkpath("dir1b"), op::RENAME, Some(cookies[0])),
        ]);
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("dir1a"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("dir1b"), op::RENAME, Some(cookies[0])),
        ]);
    }
}

#[test]
#[cfg(not(target_os="macos"))]
fn move_out_create_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "watch_dir/dir1",
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("watch_dir/dir1", "dir1b");
    tdir.create("watch_dir/dir1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("watch_dir/dir1"), op::CREATE | op::RENAME, None), // fsevent interprets a move_out as a rename event
        ]);
        panic!("move event should be a remove event; fsevent conflates rename and create events");
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("watch_dir/dir1"), op::REMOVE, None),
            (tdir.mkpath("watch_dir/dir1"), op::CREATE, None),
        ]);
    }
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
        "dir1b",
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1a");
    tdir.rename("dir1a", "dir1b");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("dir1a"), op::CREATE | op::RENAME, None),
            (tdir.mkpath("dir1b"), op::CREATE | op::RENAME, None),
        ]);
    } else if cfg!(target_os="linux") {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("dir1a"), op::CREATE, None),
            (tdir.mkpath("dir1a"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("dir1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("dir1b"), op::CHMOD, None),
        ]);
    } else {
        unimplemented!();
    }
}

#[test]
fn rename_rename_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1a",
    ]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1a", "dir1b");
    tdir.rename("dir1b", "dir1c");

    if cfg!(target_os="macos") {
        let actual = inflate_events(recv_events(&rx));
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("dir1a"), op::CREATE | op::RENAME, None),
            (tdir.mkpath("dir1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("dir1c"), op::RENAME, Some(cookies[0])),
        ]);
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(actual, vec![
            (tdir.mkpath("dir1a"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("dir1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("dir1b"), op::RENAME, Some(cookies[1])),
            (tdir.mkpath("dir1c"), op::RENAME, Some(cookies[1])),
        ]);
    }
}
