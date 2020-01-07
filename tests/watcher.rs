extern crate notify;
extern crate tempfile;

mod utils;

use notify::*;
use std::env;
use std::sync::mpsc;
use std::thread;
use tempfile::TempDir;

#[cfg(all(feature = "manual_tests", target_os = "linux"))]
use std::fs::File;
#[cfg(all(feature = "manual_tests", target_os = "linux"))]
use std::io::prelude::*;
#[cfg(all(feature = "manual_tests", target_os = "linux"))]
use std::time::{Duration, Instant};

use utils::*;

const NETWORK_PATH: &str = ""; // eg.: \\\\MY-PC\\Users\\MyName
const TEMP_DIR: &str = "temp_dir";

#[cfg(target_os = "linux")]
#[test]
fn new_inotify() {
    let (tx, _) = mpsc::channel();
    let w: Result<INotifyWatcher> = Watcher::new_raw(tx);
    assert!(w.is_ok());
}

#[cfg(target_os = "macos")]
#[test]
fn new_fsevent() {
    let (tx, _) = mpsc::channel();
    let w: Result<FsEventWatcher> = Watcher::new_raw(tx);
    assert!(w.is_ok());
}

#[test]
fn new_null() {
    let (tx, _) = mpsc::channel();
    let w: Result<NullWatcher> = Watcher::new_raw(tx);
    assert!(w.is_ok());
}

#[test]
fn new_poll() {
    let (tx, _) = mpsc::channel();
    let w: Result<PollWatcher> = Watcher::new_raw(tx);
    assert!(w.is_ok());
}

#[test]
fn new_recommended() {
    let (tx, _) = mpsc::channel();
    let w: Result<RecommendedWatcher> = Watcher::new_raw(tx);
    assert!(w.is_ok());
}

// if this test builds, it means RecommendedWatcher is Send.
#[test]
fn test_watcher_send() {
    let (tx, _) = mpsc::channel();

    let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).unwrap();

    thread::spawn(move || {
        watcher.watch(".", RecursiveMode::Recursive).unwrap();
    })
    .join()
    .unwrap();
}

// if this test builds, it means RecommendedWatcher is Sync.
#[test]
fn test_watcher_sync() {
    use std::sync::{Arc, RwLock};

    let (tx, _) = mpsc::channel();

    let watcher: RecommendedWatcher = Watcher::new_raw(tx).unwrap();
    let watcher = Arc::new(RwLock::new(watcher));

    thread::spawn(move || {
        let mut watcher = watcher.write().unwrap();
        watcher.watch(".", RecursiveMode::Recursive).unwrap();
    })
    .join()
    .unwrap();
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

        env::set_current_dir(tdir.path()).expect("failed to change working directory");

        let (tx, _) = mpsc::channel();
        let mut watcher: RecommendedWatcher =
            Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher
            .watch("dir1", RecursiveMode::Recursive)
            .expect("failed to watch directory");

        watcher
            .unwatch("dir1")
            .expect("failed to unwatch directory");

        if cfg!(not(target_os = "windows")) {
            match watcher.unwatch("dir1") {
                Err(Error::WatchNotFound) => (),
                Err(e) => panic!("{:?}", e),
                Ok(o) => panic!("{:?}", o),
            }
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

        let (tx, _) = mpsc::channel();
        let mut watcher: RecommendedWatcher =
            Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher
            .watch("file1", RecursiveMode::Recursive)
            .expect("failed to watch file");

        watcher.unwatch("file1").expect("failed to unwatch file");

        if cfg!(not(target_os = "windows")) {
            match watcher.unwatch("file1") {
                Err(Error::WatchNotFound) => (),
                Err(e) => panic!("{:?}", e),
                Ok(o) => panic!("{:?}", o),
            }
        }
    }
    if cfg!(target_os = "windows") && !NETWORK_PATH.is_empty() {
        // watch_relative_network_directory
        let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
        tdir.create("dir1");

        env::set_current_dir(tdir.path()).expect("failed to change working directory");

        let (tx, _) = mpsc::channel();
        let mut watcher: RecommendedWatcher =
            Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher
            .watch("dir1", RecursiveMode::Recursive)
            .expect("failed to watch directory");

        watcher
            .unwatch("dir1")
            .expect("failed to unwatch directory");

        if cfg!(not(target_os = "windows")) {
            match watcher.unwatch("dir1") {
                Err(Error::WatchNotFound) => (),
                Err(e) => panic!("{:?}", e),
                Ok(o) => panic!("{:?}", o),
            }
        }
    }
    if cfg!(target_os = "windows") && !NETWORK_PATH.is_empty() {
        // watch_relative_network_file
        let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
        tdir.create("file1");

        env::set_current_dir(tdir.path()).expect("failed to change working directory");

        let (tx, _) = mpsc::channel();
        let mut watcher: RecommendedWatcher =
            Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher
            .watch("file1", RecursiveMode::Recursive)
            .expect("failed to watch file");

        watcher.unwatch("file1").expect("failed to unwatch file");

        if cfg!(not(target_os = "windows")) {
            match watcher.unwatch("file1") {
                Err(Error::WatchNotFound) => (),
                Err(e) => panic!("{:?}", e),
                Ok(o) => panic!("{:?}", o),
            }
        }
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

    let (tx, _) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("dir1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    watcher
        .unwatch(tdir.mkpath("dir1"))
        .expect("failed to unwatch directory");

    if cfg!(not(target_os = "windows")) {
        match watcher.unwatch(tdir.mkpath("dir1")) {
            Err(Error::WatchNotFound) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}

#[test]
#[cfg(target_os = "windows")]
fn watch_absolute_network_file() {
    if NETWORK_PATH.is_empty() {
        return;
    }

    let tdir = TempDir::new_in(NETWORK_PATH).expect("failed to create temporary directory");
    tdir.create("file1");

    env::set_current_dir(tdir.path()).expect("failed to change working directory");

    let (tx, _) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    watcher
        .unwatch(tdir.mkpath("file1"))
        .expect("failed to unwatch file");

    if cfg!(not(target_os = "windows")) {
        match watcher.unwatch(tdir.mkpath("file1")) {
            Err(Error::WatchNotFound) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}

#[test]
#[cfg(all(feature = "manual_tests", target_os = "linux"))]
// Test preparation:
// 1. Run `sudo echo 10 > /proc/sys/fs/inotify/max_queued_events`
// 2. Uncomment the lines near "test inotify_queue_overflow" in inotify watcher
fn inotify_queue_overflow() {
    let mut max_queued_events = String::new();
    let mut f = File::open("/proc/sys/fs/inotify/max_queued_events")
        .expect("failed to open max_queued_events");
    f.read_to_string(&mut max_queued_events)
        .expect("failed to read max_queued_events");
    assert_eq!(max_queued_events.trim(), "10");

    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    for i in 0..20 {
        let filename = format!("file{}", i);
        tdir.create(&filename);
        tdir.remove(&filename);
    }

    sleep(100);

    let start = Instant::now();

    let mut rescan_found = false;
    while !rescan_found && start.elapsed().as_secs() < 5 {
        match rx.try_recv() {
            Ok(RawEvent {
                op: Ok(op::Op::RESCAN),
                ..
            }) => rescan_found = true,
            Ok(RawEvent { op: Err(e), .. }) => panic!("unexpected event err: {:?}", e),
            Ok(e) => (),
            Err(mpsc::TryRecvError::Empty) => (),
            Err(e) => panic!("unexpected channel err: {:?}", e),
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert!(rescan_found);
}

#[test]
fn watch_recursive_create_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1");
    sleep(10);
    tdir.create("dir1/file1");

    sleep_macos(100);

    watcher
        .unwatch(&tdir.mkpath("."))
        .expect("failed to unwatch directory");

    sleep_windows(100);

    tdir.create("dir1/file2");

    let actual = if cfg!(target_os = "windows") {
        // Windows may sneak a write event in there
        let mut events = recv_events(&rx);
        events.retain(|&(_, op, _)| op != op::Op::WRITE);
        events
    } else if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1/file1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1/file1"), op::Op::CLOSE_WRITE, None)
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1/file1"), op::Op::CREATE, None)
            ]
        );
    }
}

#[test]
fn watch_recursive_move() {
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

    tdir.create("dir1a/file1");
    tdir.rename("dir1a", "dir1b");
    sleep(10);
    tdir.create("dir1b/file2");

    let actual = if cfg!(target_os = "windows") {
        // Windows may sneak a write event in there
        let mut events = recv_events(&rx);
        events.retain(|&(_, op, _)| op != op::Op::WRITE);
        events
    } else if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1a/file1"), op::Op::CREATE, None),
                (
                    tdir.mkpath("dir1a"),
                    op::Op::CREATE | op::Op::RENAME,
                    Some(cookies[0])
                ), // excessive create event
                (tdir.mkpath("dir1b"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b/file2"), op::Op::CREATE, None)
            ]
        );
    } else if cfg!(target_os = "linux") {
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1a/file1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1a/file1"), op::Op::CLOSE_WRITE, None),
                (tdir.mkpath("dir1a"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b/file2"), op::Op::CREATE, None),
                (tdir.mkpath("dir1b/file2"), op::Op::CLOSE_WRITE, None)
            ]
        );
    } else {
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1a/file1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1a"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b"), op::Op::RENAME, Some(cookies[0])),
                (tdir.mkpath("dir1b/file2"), op::Op::CREATE, None)
            ]
        );
    }
}

#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn watch_recursive_move_in() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["watch_dir", "dir1a/dir1"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1a", "watch_dir/dir1b");
    sleep(10);
    tdir.create("watch_dir/dir1b/dir1/file1");

    let actual = if cfg!(target_os = "windows") {
        // Windows may sneak a write event in there
        let mut events = recv_events(&rx);
        events.retain(|&(_, op, _)| op != op::Op::WRITE);
        events
    } else if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("watch_dir/dir1b"), op::Op::RENAME, None), // fsevent interprets a move_to as a rename event
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    op::Op::CREATE,
                    None
                )
            ]
        );
        panic!("move event should be a create event");
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("watch_dir/dir1b"), op::Op::CREATE, None),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    op::Op::CREATE,
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    op::Op::CLOSE_WRITE,
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("watch_dir/dir1b"), op::Op::CREATE, None),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    op::Op::CREATE,
                    None
                ),
            ]
        );
    }
}

#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn watch_recursive_move_out() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["watch_dir/dir1a/dir1"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("watch_dir/dir1a/dir1/file1");
    tdir.rename("watch_dir/dir1a", "dir1b");
    sleep(10);
    tdir.create("dir1b/dir1/file2");

    let actual = if cfg!(target_os = "windows") {
        // Windows may sneak a write event in there
        let mut events = recv_events(&rx);
        events.retain(|&(_, op, _)| op != op::Op::WRITE);
        events
    } else if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                    op::Op::CREATE,
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1a"),
                    op::Op::CREATE | op::Op::RENAME,
                    None
                ) // excessive create event, fsevent interprets a move_out as a rename event
            ]
        );
        panic!("move event should be a remove event");
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                    op::Op::CREATE,
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                    op::Op::CLOSE_WRITE,
                    None
                ),
                (tdir.mkpath("watch_dir/dir1a"), op::Op::REMOVE, None),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                    op::Op::CREATE,
                    None
                ),
                (tdir.mkpath("watch_dir/dir1a"), op::Op::REMOVE, None),
            ]
        );
    }
}

#[test]
fn watch_nonrecursive() {
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
        .watch(tdir.mkpath("."), RecursiveMode::NonRecursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir2");
    sleep(10);
    tdir.create_all(vec!["file0", "dir1/file1", "dir2/file2"]);

    if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("dir2"), op::Op::CREATE, None),
                (tdir.mkpath("file0"), op::Op::CREATE, None),
                (tdir.mkpath("file0"), op::Op::CLOSE_WRITE, None)
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("dir2"), op::Op::CREATE, None),
                (tdir.mkpath("file0"), op::Op::CREATE, None)
            ]
        );
    }
}

#[test]
fn watch_file() {
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
        .watch(tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");
    tdir.create("file2");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("file1"), op::Op::CREATE | op::Op::WRITE, None) // excessive write create
            ]
        );
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("file1"), op::Op::WRITE, None),
                (tdir.mkpath("file1"), op::Op::CLOSE_WRITE, None)
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![(tdir.mkpath("file1"), op::Op::WRITE, None)]
        );
    }
}

#[test]
fn poll_watch_recursive_create_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    let (tx, rx) = mpsc::channel();
    let mut watcher = PollWatcher::with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.create("dir1/file1");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    watcher
        .unwatch(tdir.mkpath("."))
        .expect("failed to unwatch directory");

    tdir.create("dir1/file2");

    let mut actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        actual,
        vec![
            (tdir.mkpath("."), op::Op::WRITE, None), // parent directory gets modified
            (tdir.mkpath("dir1"), op::Op::CREATE, None),
            (tdir.mkpath("dir1/file1"), op::Op::CREATE, None)
        ]
    );
}

#[test]
#[ignore] // fails sometimes on AppVeyor
fn poll_watch_recursive_move() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1a"]);

    let (tx, rx) = mpsc::channel();
    let mut watcher = PollWatcher::with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.create("dir1a/file1");

    let mut actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        actual,
        vec![
            (tdir.mkpath("dir1a"), op::Op::WRITE, None), // parent directory gets modified
            (tdir.mkpath("dir1a/file1"), op::Op::CREATE, None),
        ]
    );

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.rename("dir1a", "dir1b");
    tdir.create("dir1b/file2");

    actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    if cfg!(target_os = "windows") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("."), op::Op::WRITE, None), // parent directory gets modified
                (tdir.mkpath("dir1a"), op::Op::REMOVE, None),
                (tdir.mkpath("dir1a/file1"), op::Op::REMOVE, None),
                (tdir.mkpath("dir1b"), op::Op::CREATE, None),
                (tdir.mkpath("dir1b"), op::Op::WRITE, None), // parent directory gets modified
                (tdir.mkpath("dir1b/file1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1b/file2"), op::Op::CREATE, None),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("."), op::Op::WRITE, None), // parent directory gets modified
                (tdir.mkpath("dir1a"), op::Op::REMOVE, None),
                (tdir.mkpath("dir1a/file1"), op::Op::REMOVE, None),
                (tdir.mkpath("dir1b"), op::Op::CREATE, None),
                (tdir.mkpath("dir1b/file1"), op::Op::CREATE, None),
                (tdir.mkpath("dir1b/file2"), op::Op::CREATE, None),
            ]
        );
    }
}

#[test]
#[ignore] // fails sometimes on AppVeyor
fn poll_watch_recursive_move_in() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["watch_dir", "dir1a/dir1"]);

    let (tx, rx) = mpsc::channel();
    let mut watcher = PollWatcher::with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.rename("dir1a", "watch_dir/dir1b");
    tdir.create("watch_dir/dir1b/dir1/file1");

    let mut actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    if cfg!(target_os = "windows") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("watch_dir"), op::Op::WRITE, None), // parent directory gets modified
                (tdir.mkpath("watch_dir/dir1b"), op::Op::CREATE, None),
                (tdir.mkpath("watch_dir/dir1b/dir1"), op::Op::CREATE, None),
                (tdir.mkpath("watch_dir/dir1b/dir1"), op::Op::WRITE, None), // extra write event
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    op::Op::CREATE,
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("watch_dir"), op::Op::WRITE, None), // parent directory gets modified
                (tdir.mkpath("watch_dir/dir1b"), op::Op::CREATE, None),
                (tdir.mkpath("watch_dir/dir1b/dir1"), op::Op::CREATE, None),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    op::Op::CREATE,
                    None
                ),
            ]
        );
    }
}

#[test]
#[ignore] // fails sometimes on AppVeyor
fn poll_watch_recursive_move_out() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["watch_dir/dir1a/dir1"]);

    let (tx, rx) = mpsc::channel();
    let mut watcher = PollWatcher::with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.create("watch_dir/dir1a/dir1/file1");

    let mut actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        actual,
        vec![
            (tdir.mkpath("watch_dir/dir1a/dir1"), op::Op::WRITE, None), // parent directory gets modified
            (
                tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                op::Op::CREATE,
                None
            ),
        ]
    );

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.rename("watch_dir/dir1a", "dir1b");
    tdir.create("dir1b/dir1/file2");

    actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        actual,
        vec![
            (tdir.mkpath("watch_dir"), op::Op::WRITE, None), // parent directory gets modified
            (tdir.mkpath("watch_dir/dir1a"), op::Op::REMOVE, None),
            (tdir.mkpath("watch_dir/dir1a/dir1"), op::Op::REMOVE, None),
            (
                tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                op::Op::REMOVE,
                None
            ),
        ]
    );
}

#[test]
fn poll_watch_nonrecursive() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    let (tx, rx) = mpsc::channel();
    let mut watcher = PollWatcher::with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(tdir.mkpath("."), RecursiveMode::NonRecursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.create_all(vec!["file1", "dir1/file1", "dir2/file1"]);

    let mut actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        actual,
        vec![
            (tdir.mkpath("."), op::Op::WRITE, None), // parent directory gets modified
            (tdir.mkpath("dir1"), op::Op::WRITE, None), // parent directory gets modified
            (tdir.mkpath("dir2"), op::Op::CREATE, None),
            (tdir.mkpath("file1"), op::Op::CREATE, None),
        ]
    );
}

#[test]
fn poll_watch_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    let (tx, rx) = mpsc::channel();
    let mut watcher = PollWatcher::with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.write("file1");
    tdir.create("file2");

    assert_eq!(
        recv_events(&rx),
        vec![(tdir.mkpath("file1"), op::Op::WRITE, None)]
    );
}

#[test]
fn watch_nonexisting() {
    let tdir1 = tempfile::Builder::new()
        .prefix("temp_dir1")
        .tempdir()
        .expect("failed to create temporary directory");
    let tdir2 = tempfile::Builder::new()
        .prefix("temp_dir2")
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir1.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");
    let result = watcher.watch(&tdir2.mkpath("non_existing"), RecursiveMode::Recursive);
    assert!(result.is_err());

    // make sure notify is still working

    sleep_windows(100);

    tdir1.create("file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            inflate_events(recv_events(&rx)),
            vec![(tdir1.mkpath("file1"), op::Op::CREATE, None),]
        );
    } else if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(tdir1.mkpath("file1"), op::Op::CREATE, None)]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir1.mkpath("file1"), op::Op::CREATE, None),
                (tdir1.mkpath("file1"), op::Op::CLOSE_WRITE, None)
            ]
        );
    }
}

#[test]
fn unwatch_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(10);

    let (tx, _) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    match watcher.unwatch(&tdir.mkpath("file1")) {
        Ok(_) => (),
        Err(e) => panic!("{:?}", e),
    }
}

#[test]
fn unwatch_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(10);

    let (tx, _) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("dir1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    match watcher.unwatch(&tdir.mkpath("dir1")) {
        Ok(_) => (),
        Err(e) => panic!("{:?}", e),
    }
}

#[test]
#[cfg_attr(target_os = "windows", ignore)]
fn unwatch_nonexisting() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, _) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");

    match watcher.unwatch(&tdir.mkpath("file1")) {
        Err(Error::WatchNotFound) => (),
        Err(e) => panic!("{:?}", e),
        Ok(o) => panic!("{:?}", o),
    }
}

#[test]
fn self_delete_file() {
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
        .watch(&tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    sleep_windows(100);

    tdir.remove("file1");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "windows") {
        assert_eq!(actual, vec![(tdir.mkpath("file1"), op::Op::REMOVE, None),]);
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![(tdir.mkpath("file1"), op::Op::CREATE | op::Op::REMOVE, None),]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("file1"), op::Op::CHMOD, None),
                (tdir.mkpath("file1"), op::Op::REMOVE, None),
            ]
        );
    }

    if cfg!(not(any(target_os = "windows", target_os = "macos"))) {
        tdir.create("file1");

        assert_eq!(recv_events(&rx), vec![]);

        match watcher.unwatch(&tdir.mkpath("file1")) {
            Err(Error::WatchNotFound) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}

#[test]
#[cfg_attr(target_os = "windows", ignore)]
fn self_delete_directory() {
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
        .watch(&tdir.mkpath("dir1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.remove("dir1");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "windows") {
        assert_eq!(actual, vec![(tdir.mkpath("dir1"), Op::empty(), None),]);
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![(tdir.mkpath("dir1"), op::Op::CREATE | op::Op::REMOVE, None),]
        );
    } else {
        assert_eq!(actual, vec![(tdir.mkpath("dir1"), op::Op::REMOVE, None),]);
    }

    tdir.create("dir1");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1"), op::Op::CREATE | op::Op::REMOVE, None), // excessive remove event
            ]
        );
    } else {
        assert_eq!(actual, vec![]);
    }

    if cfg!(not(any(target_os = "windows", target_os = "macos"))) {
        match watcher.unwatch(&tdir.mkpath("dir1")) {
            Err(Error::WatchNotFound) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}

#[test]
#[cfg_attr(target_os = "windows", ignore)]
fn self_rename_file() {
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
        .watch(&tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    sleep_windows(100);

    tdir.rename("file1", "file2");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("file1"), op::Op::CREATE | op::Op::RENAME, None), // excessive create event
            ]
        );
    } else {
        assert_eq!(actual, vec![(tdir.mkpath("file1"), op::Op::RENAME, None),]);
    }

    tdir.write("file2");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "windows") {
        assert_eq!(actual, vec![]);
        panic!("windows back-end should update file watch path");
    } else if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("file1"), op::Op::WRITE, None), // path doesn't get updated
                (tdir.mkpath("file1"), op::Op::CLOSE_WRITE, None), // path doesn't get updated
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
            (tdir.mkpath("file1"), op::Op::WRITE, None), // path doesn't get updated
        ]
        );
    }

    tdir.create("file1");

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(
            recv_events(&rx),
            vec![
                (tdir.mkpath("file1"), op::Op::CREATE | op::Op::RENAME, None), // excessive rename event
            ]
        );
    } else {
        assert_eq!(recv_events(&rx), vec![]);
    }

    watcher
        .unwatch(&tdir.mkpath("file1"))
        .expect("failed to unwatch file"); // use old path to unwatch

    let result = watcher.unwatch(&tdir.mkpath("file1"));
    match result {
        Err(Error::WatchNotFound) => (),
        Err(e) => panic!("{:?}", e),
        Ok(o) => panic!("{:?}", o),
    }
}

#[test]
fn self_rename_directory() {
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
        .watch(&tdir.mkpath("dir1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1", "dir2");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "windows") {
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1"), op::Op::CREATE | op::Op::RENAME, None), // excessive create event
            ]
        );
    } else {
        assert_eq!(actual, vec![(tdir.mkpath("dir1"), op::Op::RENAME, None),]);
    }

    tdir.create("dir2/file1");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1/file1"), op::Op::CREATE, None), // path doesn't get updated
                (tdir.mkpath("dir1/file1"), op::Op::CLOSE_WRITE, None)
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
            (tdir.mkpath("dir1/file1"), op::Op::CREATE, None) // path doesn't get updated
        ]
        );
    }

    tdir.create("dir1");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1"), op::Op::CREATE | op::Op::RENAME, None), // excessive rename event
            ]
        );
    } else {
        assert_eq!(actual, vec![]);
    }

    watcher
        .unwatch(&tdir.mkpath("dir1"))
        .expect("failed to unwatch directory"); // use old path to unwatch

    let result = watcher.unwatch(&tdir.mkpath("dir1"));
    if cfg!(target_os = "windows") {
        match result {
            Err(e) => panic!("{:?}", e),
            Ok(()) => (),
        }
    } else {
        match result {
            Err(Error::WatchNotFound) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}

#[test]
fn parent_rename_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1/file1"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("dir1/file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    sleep_windows(100);

    tdir.rename("dir1", "dir2");

    assert_eq!(recv_events(&rx), vec![]);

    tdir.write("dir2/file1");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1/file1"), op::Op::WRITE, None), // path doesn't get updated
                (tdir.mkpath("dir1/file1"), op::Op::CLOSE_WRITE, None)
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
            (tdir.mkpath("dir1/file1"), op::Op::WRITE, None) // path doesn't get updated
        ]
        );
    }

    tdir.create("dir1/file1");

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(
            recv_events(&rx),
            vec![(tdir.mkpath("dir1/file1"), op::Op::CREATE, None),]
        );
    } else {
        assert_eq!(recv_events(&rx), vec![]);
    }

    watcher
        .unwatch(&tdir.mkpath("dir1/file1"))
        .expect("failed to unwatch file"); // use old path to unwatch

    let result = watcher.unwatch(&tdir.mkpath("dir1/file1"));
    if cfg!(target_os = "windows") {
        match result {
            Err(e) => panic!("{:?}", e),
            Ok(()) => (),
        }
    } else {
        match result {
            Err(Error::WatchNotFound) => (),
            Err(e) => panic!("{:?}", e),
            Ok(o) => panic!("{:?}", o),
        }
    }
}

#[test]
#[cfg_attr(target_os = "windows", ignore)]
fn parent_rename_directory() {
    // removing the parent directory doesn't work on windows
    if cfg!(target_os = "windows") {
        panic!("cannot remove parent directory on windows");
    }

    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1/watch_dir"]);

    sleep_macos(10);

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher =
        Watcher::new_raw(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("dir1/watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("dir1", "dir2");

    assert_eq!(recv_events(&rx), vec![]);

    tdir.create("dir2/watch_dir/file1");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1/watch_dir/file1"), op::Op::CREATE, None), // path doesn't get updated
                (
                    tdir.mkpath("dir1/watch_dir/file1"),
                    op::Op::CLOSE_WRITE,
                    None
                ), // path doesn't get updated
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (tdir.mkpath("dir1/watch_dir/file1"), op::Op::CREATE, None), // path doesn't get updated
            ]
        );
    }

    tdir.create("dir1/watch_dir");

    let actual = if cfg!(target_os = "macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(
            actual,
            vec![(tdir.mkpath("dir1/watch_dir"), op::Op::CREATE, None),]
        );
    } else {
        assert_eq!(actual, vec![]);
    }

    watcher
        .unwatch(&tdir.mkpath("dir1/watch_dir"))
        .expect("failed to unwatch directory"); // use old path to unwatch

    let result = watcher.unwatch(&tdir.mkpath("dir1/watch_dir"));
    match result {
        Err(Error::WatchNotFound) => (),
        Err(e) => panic!("{:?}", e),
        Ok(o) => panic!("{:?}", o),
    }
}
