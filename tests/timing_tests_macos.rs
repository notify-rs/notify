#![cfg(target_os = "macos")]
#![cfg(feature = "timing_tests")]

use std::time::Duration;

use crossbeam_channel::unbounded;
use notify::*;

use utils::*;
mod utils;

const TEMP_DIR: &str = "temp_dir";
const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn fsevents_create_delete_file_0() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");
    tdir.remove("file1");

    let actual = recv_events_with_timeout(&rx, TIMEOUT);
    if actual
        == vec![
            (
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None,
            ),
            (
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None,
            ),
        ]
    {
        panic!("single CREATE | REMOVE event");
    } else {
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
                (
                    tdir.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
            ]
        );
    }
}

#[test]
fn fsevents_create_delete_file_1() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(
        recv_events_with_timeout(&rx, Duration::from_secs(1)),
        vec![(
            tdir.mkpath("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );

    tdir.remove("file1");

    let actual = recv_events_with_timeout(&rx, TIMEOUT);
    if actual
        == vec![
            (
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None,
            ),
            (
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None,
            ),
        ]
    {
        panic!("excessive CREATE event");
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
        );
    }
}

#[test]
fn fsevents_create_delete_file_2() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(
        recv_events_with_timeout(&rx, Duration::from_secs(2)),
        vec![(
            tdir.mkpath("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );

    tdir.remove("file1");

    let actual = recv_events_with_timeout(&rx, TIMEOUT);
    if actual
        == vec![
            (
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None,
            ),
            (
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None,
            ),
        ]
    {
        panic!("excessive CREATE event");
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
        );
    }
}

#[test]
fn fsevents_create_delete_file_4() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(
        recv_events_with_timeout(&rx, Duration::from_secs(4)),
        vec![(
            tdir.mkpath("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );

    tdir.remove("file1");

    let actual = recv_events_with_timeout(&rx, TIMEOUT);
    if actual
        == vec![
            (
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None,
            ),
            (
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None,
            ),
        ]
    {
        panic!("excessive CREATE event");
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
        );
    }
}

#[test]
fn fsevents_create_delete_file_8() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(
        recv_events_with_timeout(&rx, Duration::from_secs(8)),
        vec![(
            tdir.mkpath("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );

    tdir.remove("file1");

    let actual = recv_events_with_timeout(&rx, TIMEOUT);
    if actual
        == vec![
            (
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None,
            ),
            (
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None,
            ),
        ]
    {
        panic!("excessive CREATE event");
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
        );
    }
}

#[test]
fn fsevents_create_delete_file_16() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(
        recv_events_with_timeout(&rx, Duration::from_secs(16)),
        vec![(
            tdir.mkpath("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );

    tdir.remove("file1");

    let actual = recv_events_with_timeout(&rx, TIMEOUT);
    if actual
        == vec![
            (
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None,
            ),
            (
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None,
            ),
        ]
    {
        panic!("excessive CREATE event");
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
        );
    }
}

#[test]
fn fsevents_create_delete_file_32() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(
        recv_events_with_timeout(&rx, Duration::from_secs(32)),
        vec![(
            tdir.mkpath("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );

    tdir.remove("file1");

    let actual = recv_events_with_timeout(&rx, TIMEOUT);
    if actual
        == vec![
            (
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None,
            ),
            (
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None,
            ),
        ]
    {
        panic!("excessive CREATE event");
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
        );
    }
}

#[test]
fn fsevents_create_delete_file_64() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.create("file1");

    assert_eq!(
        recv_events_with_timeout(&rx, Duration::from_secs(64)),
        vec![(
            tdir.mkpath("file1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );

    tdir.remove("file1");

    let actual = recv_events_with_timeout(&rx, TIMEOUT);
    if actual
        == vec![
            (
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None,
            ),
            (
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None,
            ),
        ]
    {
        panic!("excessive CREATE event");
    } else {
        assert_eq!(
            actual,
            vec![(
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
        );
    }
}

#[test]
fn fsevents_rename_rename_file_0() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1a"]);

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1a", "file1b");
    tdir.rename("file1b", "file1c");

    let actual = recv_events(&rx);
    let cookies = extract_cookies(&actual);
    assert_eq!(cookies.len(), 1);
    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("file1a"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                None
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1c"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
        ]
    );
}

#[test]
fn fsevents_rename_rename_file_10() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1a"]);

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1a", "file1b");
    sleep(10);
    tdir.rename("file1b", "file1c");

    let actual = recv_events(&rx);
    let cookies = extract_cookies(&actual);
    assert_eq!(cookies.len(), 2);
    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("file1a"),
                EventKind::Create(event::CreateKind::Any),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
            (
                tdir.mkpath("file1c"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
        ]
    );
}

#[test]
fn fsevents_rename_rename_file_20() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1a"]);

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1a", "file1b");
    sleep(20);
    tdir.rename("file1b", "file1c");

    let actual = recv_events(&rx);
    let cookies = extract_cookies(&actual);
    assert_eq!(cookies.len(), 2);
    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("file1a"),
                EventKind::Create(event::CreateKind::Any),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
            (
                tdir.mkpath("file1c"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
        ]
    );
}

#[test]
fn fsevents_rename_rename_back_file_0() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1a"]);

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1a", "file1b");
    tdir.rename("file1b", "file1a");

    let actual = recv_events(&rx);
    let cookies = extract_cookies(&actual);
    assert_eq!(cookies.len(), 1);
    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Create(event::CreateKind::Any),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
        ]
    );
}

#[test]
fn fsevents_rename_rename_back_file_10() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1a"]);

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1a", "file1b");
    sleep(10);
    tdir.rename("file1b", "file1a");

    let actual = recv_events(&rx);
    let cookies = extract_cookies(&actual);
    assert_eq!(cookies.len(), 2);
    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("file1a"),
                EventKind::Create(event::CreateKind::Any),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Create(event::CreateKind::Any),
                Some(cookies[1])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
        ]
    );
}

#[test]
fn fsevents_rename_rename_back_file_20() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1a"]);

    sleep(10);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1a", "file1b");
    sleep(20);
    tdir.rename("file1b", "file1a");

    let actual = recv_events(&rx);
    let cookies = extract_cookies(&actual);
    assert_eq!(cookies.len(), 2);
    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("file1a"),
                EventKind::Create(event::CreateKind::Any),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Create(event::CreateKind::Any),
                Some(cookies[1])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
        ]
    );
}

#[test]
fn fsevents_rename_rename_back_file_sleep() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    tdir.create_all(vec!["file1a"]);

    sleep(40_000);

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    tdir.rename("file1a", "file1b");
    sleep(10);
    tdir.rename("file1b", "file1a");

    let actual = recv_events(&rx);
    let cookies = extract_cookies(&actual);
    assert_eq!(cookies.len(), 2);
    assert_eq!(
        actual,
        vec![
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[0])
            ),
            (
                tdir.mkpath("file1b"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
            (
                tdir.mkpath("file1a"),
                EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                Some(cookies[1])
            ),
        ]
    );
}
