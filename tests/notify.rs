extern crate notify;
extern crate tempfile;

mod utils;

use notify::*;
use std::path::PathBuf;
use std::sync::mpsc;

use utils::*;

const TEMP_DIR: &str = "temp_dir";

#[test]
fn test_inflate_events() {
    assert_eq!(
        inflate_events(vec![
            (PathBuf::from("file1"), op::Op::CREATE, None),
            (PathBuf::from("file1"), op::Op::WRITE, None),
        ]),
        vec![(PathBuf::from("file1"), op::Op::CREATE | op::Op::WRITE, None),]
    );

    assert_eq!(
        inflate_events(vec![
            (PathBuf::from("file1"), op::Op::RENAME, Some(1)),
            (PathBuf::from("file1"), op::Op::RENAME, Some(2)),
        ]),
        vec![
            (PathBuf::from("file1"), op::Op::RENAME, Some(1)),
            (PathBuf::from("file1"), op::Op::RENAME, Some(2)),
        ]
    );
}

#[test]
fn create_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    // macOS FsEvent needs some time to discard old events from its log.
    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![(tdir.mkpath("file1"), op::Op::CREATE, None),]
        );
    } else if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(tdir.mkpath("file1"), op::Op::CREATE, None)]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("file1"), op::Op::CREATE, None),
                (tdir.mkpath("file1"), op::Op::CLOSE_WRITE, None)
            ]
        );
    }
}

#[test]
fn write_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![
                (tdir.mkpath("file1"), op::Op::CREATE | op::Op::WRITE, None), // excessive create event
            ]
        );
    } else if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(tdir.mkpath("file1"), op::Op::WRITE, None)]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("file1"), op::Op::WRITE, None),
                (tdir.mkpath("file1"), op::Op::CLOSE_WRITE, None)
            ]
        );
    }
}

#[test]
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
fn modify_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.chmod("file1");

    sleep_macos(10);

    if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(tdir.mkpath("file1"), op::Op::WRITE, None),]
        );
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![(tdir.mkpath("file1"), op::Op::CHMOD | op::Op::CREATE, None),]
        );
        panic!("macos cannot distinguish between chmod and create");
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![(tdir.mkpath("file1"), op::Op::CHMOD, None),]
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

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.remove("file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![
                (tdir.mkpath("file1"), op::Op::CREATE | op::Op::REMOVE, None), // excessive create event
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![(tdir.mkpath("file1"), op::Op::REMOVE, None),]
        );
    }
}

#[test]
fn rename_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1a"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("file1a", "file1b");

    if cfg!(target_os = "macos") {
        let actual = inflate_events(recv_events(&rx));
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("file1a"),
                    op::Op::CREATE | op::Op::RENAME,
                    Some(cookies[0])
                ), // excessive create event
                (tdir.mkpath("file1b"), op::Op::RENAME, Some(cookies[0]))
            ]
        );
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("file1a"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("file1b"), op::Op::RENAME, Some(cookies[0]))
            ]
        );
    }
}

#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn move_out_create_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["watch_dir/file1"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("watch_dir/file1", "file1b");
    tdir.create("watch_dir/file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![
                (
                    tdir.mkpath("watch_dir/file1"),
                    op::Op::CREATE | op::Op::RENAME,
                    None
                ), // fsevent interprets a move_out as a rename event
            ]
        );
        panic!("move event should be a remove event; fsevent conflates rename and create events");
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("watch_dir/file1"), op::Op::REMOVE, None),
                (tdir.mkpath("watch_dir/file1"), op::Op::CREATE, None),
                (tdir.mkpath("watch_dir/file1"), op::Op::CLOSE_WRITE, None),
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("watch_dir/file1"), op::Op::REMOVE, None),
                (tdir.mkpath("watch_dir/file1"), op::Op::CREATE, None),
            ]
        );
    }
}

#[test]
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
fn create_write_modify_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("file1");
    tdir.write("file1");
    tdir.chmod("file1");

    if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("file1"), op::Op::CREATE, None),
                (tdir.mkpath("file1"), op::Op::WRITE, None),
                (tdir.mkpath("file1"), op::Op::WRITE, None),
            ]
        );
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![(
                tdir.mkpath("file1"),
                op::Op::CHMOD | op::Op::CREATE | op::Op::WRITE,
                None
            ),]
        );
        panic!("macos cannot distinguish between chmod and create");
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("file1"), op::Op::CREATE, None),
                (tdir.mkpath("file1"), op::Op::CLOSE_WRITE, None),
                (tdir.mkpath("file1"), op::Op::WRITE, None),
                (tdir.mkpath("file1"), op::Op::CLOSE_WRITE, None),
                (tdir.mkpath("file1"), op::Op::CHMOD, None),
            ]
        );
    }
}

#[test]
fn create_rename_overwrite_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1b"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("file1a");
    tdir.rename("file1a", "file1b");

    let actual = recv_events(&rx);

    if cfg!(target_os = "windows") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("file1a"), op::Op::CREATE, None),
                (tdir.mkpath("file1b"), op::Op::REMOVE, None),
                (tdir.mkpath("file1a"), op::Op::RENAME, Some(1)),
                (tdir.mkpath("file1b"), op::Op::RENAME, Some(1))
            ]
        );
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(actual),
            vec![
                (tdir.mkpath("file1a"), op::Op::CREATE | op::Op::RENAME, None),
                (tdir.mkpath("file1b"), op::Op::CREATE | op::Op::RENAME, None)
            ]
        );
    } else {
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("file1a"), op::Op::CREATE, None),
                (tdir.mkpath("file1a"), op::Op::CLOSE_WRITE, None),
                (tdir.mkpath("file1a"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("file1b"), op::Op::RENAME, Some(cookies[0]))
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

    tdir.create_all(vec!["file1a"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("file1a", "file1b");
    tdir.rename("file1b", "file1c");

    if cfg!(target_os = "macos") {
        let actual = inflate_events(recv_events(&rx));
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("file1a"), op::Op::CREATE | op::Op::RENAME, None),
                (tdir.mkpath("file1b"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("file1c"), op::Op::RENAME, Some(cookies[0]))
            ]
        );
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("file1a"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("file1b"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("file1b"), op::Op::RENAME, Some(cookies[1])),
                (tdir.mkpath("file1c"), op::Op::RENAME, Some(cookies[1]))
            ]
        );
    }
}

#[test]
fn create_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    assert_eq!(actual, vec![(tdir.mkpath("dir1"), op::Op::CREATE, None),]);
}

// https://github.com/passcod/notify/issues/124
#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn create_directory_watch_subdirectories() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1");
    tdir.create("dir1/dir2");

    sleep(100);

    tdir.create("dir1/dir2/file1");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1/dir2/file1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1/dir2/file1"), op::Op::CLOSE_WRITE, None),
            ]
        );
    } else if cfg!(target_os = "windows") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1/dir2"), op::Op::CREATE, None),
                (tdir.mkpath("dir1/dir2/file1"), op::Op::CREATE, None),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1/dir2/file1"), op::Op::CREATE, None),
            ]
        );
    }
}

#[test]
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
fn modify_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.chmod("dir1");

    if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(tdir.mkpath("dir1"), op::Op::WRITE, None),]
        );
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![(tdir.mkpath("dir1"), op::Op::CHMOD | op::Op::CREATE, None),]
        );
        panic!("macos cannot distinguish between chmod and create");
    } else {
        // TODO: emit chmod event only once
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("dir1"), op::Op::CHMOD, None),
                (tdir.mkpath("dir1"), op::Op::CHMOD, None),
            ]
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

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.remove("dir1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![
                (tdir.mkpath("dir1"), op::Op::CREATE | op::Op::REMOVE, None), // excessive create event
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![(tdir.mkpath("dir1"), op::Op::REMOVE, None),]
        );
    }
}

#[test]
fn rename_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1a"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1a", "dir1b");

    if cfg!(target_os = "macos") {
        let actual = inflate_events(recv_events(&rx));
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1a"),
                    op::Op::CREATE | op::Op::RENAME,
                    Some(cookies[0])
                ), // excessive create event
                (tdir.mkpath("dir1b"), op::Op::RENAME, Some(cookies[0])),
            ]
        );
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1a"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b"), op::Op::RENAME, Some(cookies[0])),
            ]
        );
    }
}

#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn move_out_create_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["watch_dir/dir1"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("watch_dir/dir1", "dir1b");
    tdir.create("watch_dir/dir1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![
                (
                    tdir.mkpath("watch_dir/dir1"),
                    op::Op::CREATE | op::Op::RENAME,
                    None
                ), // fsevent interprets a move_out as a rename event
            ]
        );
        panic!("move event should be a remove event; fsevent conflates rename and create events");
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("watch_dir/dir1"), op::Op::REMOVE, None),
                (tdir.mkpath("watch_dir/dir1"), op::Op::CREATE, None),
            ]
        );
    }
}

// https://github.com/passcod/notify/issues/124
// fails consistently on windows, macos -- tbd?
#[test]
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
fn move_in_directory_watch_subdirectories() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["watch_dir", "dir1/dir2"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1", "watch_dir/dir1");

    sleep(100);

    tdir.create("watch_dir/dir1/dir2/file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![(
                tdir.mkpath("watch_dir/dir1"),
                op::Op::CREATE | op::Op::RENAME,
                None
            ),]
        );
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("watch_dir/dir1"), op::Op::CREATE, None),
                (
                    tdir.mkpath("watch_dir/dir1/dir2/file1"),
                    op::Op::CREATE,
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1/dir2/file1"),
                    op::Op::CLOSE_WRITE,
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("watch_dir/dir1"), op::Op::CREATE, None),
                (
                    tdir.mkpath("watch_dir/dir1/dir2/file1"),
                    op::Op::CREATE,
                    None
                ),
            ]
        );
    }
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

    tdir.create_all(vec!["dir1b"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1a");
    tdir.rename("dir1a", "dir1b");

    if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![
                (tdir.mkpath("dir1a"), op::Op::CREATE | op::Op::RENAME, None),
                (tdir.mkpath("dir1b"), op::Op::CREATE | op::Op::RENAME, None),
            ]
        );
    } else if cfg!(target_os = "linux") {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1a"), op::Op::CREATE, None),
                (tdir.mkpath("dir1a"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b"), op::Op::CHMOD, None),
            ]
        );
    } else {
        unimplemented!();
    }
}

#[test]
fn rename_rename_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1a"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1a", "dir1b");
    tdir.rename("dir1b", "dir1c");

    if cfg!(target_os = "macos") {
        let actual = inflate_events(recv_events(&rx));
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1a"), op::Op::CREATE | op::Op::RENAME, None),
                (tdir.mkpath("dir1b"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1c"), op::Op::RENAME, Some(cookies[0])),
            ]
        );
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1a"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b"), op::Op::RENAME, Some(cookies[1])),
                (tdir.mkpath("dir1c"), op::Op::RENAME, Some(cookies[1])),
            ]
        );
    }
}
