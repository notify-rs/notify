use std::{env, path::Path, thread};

use crossbeam_channel::unbounded;
use notify::*;

use utils::*;
mod utils;

const TEMP_DIR: &str = "temp_dir";

#[cfg(target_os = "linux")]
#[test]
fn new_inotify() {
    let (tx, _) = unbounded();
    let w: Result<INotifyWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[cfg(target_os = "macos")]
#[test]
fn new_fsevent() {
    let (tx, _) = unbounded();
    let w: Result<FsEventWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[test]
fn new_null() {
    let (tx, _) = unbounded();
    let w: Result<NullWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[test]
fn new_poll() {
    let (tx, _) = unbounded();
    let w: Result<PollWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[test]
fn new_recommended() {
    let (tx, _) = unbounded();
    let w: Result<RecommendedWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

// if this test builds, it means RecommendedWatcher is Send.
#[test]
fn test_watcher_send() {
    let (tx, _) = unbounded();

    let mut watcher: RecommendedWatcher = Watcher::new(tx).unwrap();

    thread::spawn(move || {
        watcher
            .watch(Path::new("."), RecursiveMode::Recursive)
            .unwrap();
    })
    .join()
    .unwrap();
}

// if this test builds, it means RecommendedWatcher is Sync.
#[test]
fn test_watcher_sync() {
    use std::sync::{Arc, RwLock};

    let (tx, _) = unbounded();

    let watcher: RecommendedWatcher = Watcher::new(tx).unwrap();
    let watcher = Arc::new(RwLock::new(watcher));

    thread::spawn(move || {
        let mut watcher = watcher.write().unwrap();
        watcher
            .watch(Path::new("."), RecursiveMode::Recursive)
            .unwrap();
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

        let (tx, _) = unbounded();
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
    {
        // watch_relative_file
        let tdir = tempfile::Builder::new()
            .prefix(TEMP_DIR)
            .tempdir()
            .expect("failed to create temporary directory");
        tdir.create("file1");

        env::set_current_dir(tdir.path()).expect("failed to change working directory");

        let (tx, _) = unbounded();
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
}

#[test]
fn watch_recursive_create_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep_macos(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
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
        events.retain(|(_, ref kind, _)| {
            matches!(
                kind,
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any))
            )
        });
        events
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                )
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                )
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

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1a/file1");
    tdir.rename("dir1a", "dir1b");
    sleep(10);
    tdir.create("dir1b/file2");

    let actual = if cfg!(target_os = "windows") {
        // Windows may sneak a write event in there
        let mut events = recv_events(&rx);
        events.retain(|(_, ref kind, _)| {
            matches!(
                kind,
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any))
            )
        });
        events
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1a/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Create(event::CreateKind::Any),
                    Some(cookies[0])
                ), // excessive create event
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                ),
                (
                    tdir.mkpath("dir1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                ),
                (
                    tdir.mkpath("dir1b/file2"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                )
            ]
        );
    } else if cfg!(target_os = "linux") {
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1a/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1a/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ),
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                ),
                (
                    tdir.mkpath("dir1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                ),
                (
                    tdir.mkpath("dir1b/file2"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1b/file2"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                )
            ]
        );
    } else {
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1a/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                ),
                (
                    tdir.mkpath("dir1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                ),
                (
                    tdir.mkpath("dir1b/file2"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                )
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

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
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
        events.retain(|(_, ref kind, _)| {
            matches!(
                kind,
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any))
            )
        });
        events
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir/dir1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ), // fsevent interprets a move_to as a rename event
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                )
            ]
        );
        panic!("move event should be a create event");
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir/dir1b"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir/dir1b"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
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

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("watch_dir/dir1a/dir1/file1");
    tdir.rename("watch_dir/dir1a", "dir1b");
    sleep(10);
    tdir.create("dir1b/dir1/file2");

    let actual = if cfg!(target_os = "windows") {
        // Windows may sneak a write event in there
        let mut events = recv_events(&rx);
        events.retain(|(_, ref kind, _)| {
            matches!(
                kind,
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any))
            )
        });
        events
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1a"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // excessive create event
                (
                    tdir.mkpath("watch_dir/dir1a"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ) // fsevent interprets a move_out as a rename event
            ]
        );
        panic!("move event should be a remove event");
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1a"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1a"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
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

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::NonRecursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir2");
    sleep(10);
    tdir.create_all(vec!["file0", "dir1/file1", "dir2/file2"]);

    if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("dir2"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("file0"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("file0"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                )
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("dir2"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("file0"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                )
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

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");
    tdir.create("file2");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // excessive write create
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ),
            ]
        );
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                )
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("file1"),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            )]
        );
    }
}

#[test]
fn poll_watch_recursive_create_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    let (tx, rx) = unbounded();
    let mut watcher = poll_with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.create("dir1/file1");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    watcher
        .unwatch(&tdir.mkpath("."))
        .expect("failed to unwatch directory");

    tdir.create("dir1/file2");

    let mut actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("."),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            ), // parent directory gets modified
            (
                tdir.mkpath("dir1"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),
            (
                tdir.mkpath("dir1/file1"),
                EventKind::Create(event::CreateKind::Any),
                None
            )
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

    let (tx, rx) = unbounded();
    let mut watcher = poll_with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.create("dir1a/file1");

    let mut actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("dir1a"),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            ), // parent directory gets modified
            (
                tdir.mkpath("dir1a/file1"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),
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
                (
                    tdir.mkpath("."),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // parent directory gets modified
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1a/file1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1b"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1b"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // parent directory gets modified
                (
                    tdir.mkpath("dir1b/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1b/file2"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("."),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // parent directory gets modified
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1a/file1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1b"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1b/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1b/file2"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
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

    let (tx, rx) = unbounded();
    let mut watcher = poll_with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(&tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
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
                (
                    tdir.mkpath("watch_dir"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // parent directory gets modified
                (
                    tdir.mkpath("watch_dir/dir1b"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // extra write event
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("watch_dir"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // parent directory gets modified
                (
                    tdir.mkpath("watch_dir/dir1b"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1b/dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
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

    let (tx, rx) = unbounded();
    let mut watcher = poll_with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(&tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.create("watch_dir/dir1a/dir1/file1");

    let mut actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("watch_dir/dir1a/dir1"),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            ), // parent directory gets modified
            (
                tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                EventKind::Create(event::CreateKind::Any),
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
            (
                tdir.mkpath("watch_dir"),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            ), // parent directory gets modified
            (
                tdir.mkpath("watch_dir/dir1a"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),
            (
                tdir.mkpath("watch_dir/dir1a/dir1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),
            (
                tdir.mkpath("watch_dir/dir1a/dir1/file1"),
                EventKind::Remove(event::RemoveKind::Any),
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

    let (tx, rx) = unbounded();
    let mut watcher = poll_with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::NonRecursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.create_all(vec!["file1", "dir1/file1", "dir2/file1"]);

    let mut actual = recv_events(&rx);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("."),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            ), // parent directory gets modified
            (
                tdir.mkpath("dir1"),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            ), // parent directory gets modified
            (
                tdir.mkpath("dir2"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),
            (
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),
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

    let (tx, rx) = unbounded();
    let mut watcher = poll_with_delay_ms(tx, 50).expect("failed to create poll watcher");
    watcher
        .watch(&tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep(1100); // PollWatcher has only a resolution of 1 second

    tdir.write("file1");
    tdir.create("file2");

    assert_eq!(
        recv_events(&rx),
        vec![(
            tdir.mkpath("file1"),
            EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
            None
        )]
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

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
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
            (recv_events(&rx)),
            vec![(
                tdir1.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),]
        );
    } else if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir1.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None
            )]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir1.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir1.mkpath("file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                )
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

    let (tx, _) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
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

    let (tx, _) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
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

    let (tx, _) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");

    match watcher.unwatch(&tdir.mkpath("file1")) {
        Err(Error {
            kind: ErrorKind::WatchNotFound,
            ..
        }) => (),
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

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    sleep_windows(100);

    tdir.remove("file1");

    let actual = recv_events(&rx);

    if cfg!(target_os = "windows") {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
        );
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Metadata(event::MetadataKind::Any)),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
            ]
        );
    }

    if cfg!(not(any(target_os = "windows", target_os = "macos"))) {
        tdir.create("file1");

        assert_eq!(recv_events(&rx), vec![]);

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

#[test]
#[cfg_attr(target_os = "windows", ignore)]
fn self_delete_directory() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1"]);

    sleep_macos(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("dir1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.remove("dir1");

    let actual = recv_events(&rx);

    if cfg!(target_os = "windows") {
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("dir1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
        );
    }

    tdir.create("dir1");

    let actual = recv_events(&rx);

    if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ), // excessive remove event
            ]
        );
    } else {
        assert_eq!(actual, vec![]);
    }

    if cfg!(not(any(target_os = "windows", target_os = "macos"))) {
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
#[cfg_attr(target_os = "windows", ignore)]
fn self_rename_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1"]);

    sleep_macos(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    sleep_windows(100);

    tdir.rename("file1", "file2");

    let actual = recv_events(&rx);

    if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // excessive create event
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("file1"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                None
            ),]
        );
    }

    tdir.write("file2");

    let actual = recv_events(&rx);

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
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // path doesn't get updated
                (
                    tdir.mkpath("file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ), // path doesn't get updated
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // path doesn't get updated
            ]
        );
    }

    tdir.create("file1");

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ), // excessive rename event
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
        Err(Error {
            kind: ErrorKind::WatchNotFound,
            ..
        }) => (),
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

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("dir1"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1", "dir2");

    let actual = recv_events(&rx);

    if cfg!(target_os = "windows") {
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // excessive create event
                (
                    tdir.mkpath("dir1"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("dir1"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                None
            ),]
        );
    }

    tdir.create("dir2/file1");

    let actual = recv_events(&rx);

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // path doesn't get updated
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                )
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ) // path doesn't get updated
            ]
        );
    }

    tdir.create("dir1");

    let actual = recv_events(&rx);

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ), // excessive rename event
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
fn parent_rename_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["dir1/file1"]);

    sleep_macos(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("dir1/file1"), RecursiveMode::Recursive)
        .expect("failed to watch file");

    sleep_windows(100);

    tdir.rename("dir1", "dir2");

    assert_eq!(recv_events(&rx), vec![]);

    tdir.write("dir2/file1");

    let actual = recv_events(&rx);

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ), // path doesn't get updated
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                )
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1/file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ) // path doesn't get updated
            ]
        );
    }

    tdir.create("dir1/file1");

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("dir1/file1"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),]
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

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("dir1/watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("dir1", "dir2");

    assert_eq!(recv_events(&rx), vec![]);

    tdir.create("dir2/watch_dir/file1");

    let actual = recv_events(&rx);

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(actual, vec![]);
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1/watch_dir/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // path doesn't get updated
                (
                    tdir.mkpath("dir1/watch_dir/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ), // path doesn't get updated
            ]
        );
    } else {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1/watch_dir/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // path doesn't get updated
            ]
        );
    }

    tdir.create("dir1/watch_dir");

    let actual = recv_events(&rx);

    if cfg!(target_os = "macos") {
        // macos doesn't watch files, but paths
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("dir1/watch_dir"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),]
        );
    } else {
        assert_eq!(actual, vec![]);
    }

    watcher
        .unwatch(&tdir.mkpath("dir1/watch_dir"))
        .expect("failed to unwatch directory"); // use old path to unwatch

    let result = watcher.unwatch(&tdir.mkpath("dir1/watch_dir"));
    match result {
        Err(Error {
            kind: ErrorKind::WatchNotFound,
            ..
        }) => (),
        Err(e) => panic!("{:?}", e),
        Ok(o) => panic!("{:?}", o),
    }
}
