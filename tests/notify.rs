use crossbeam_channel::unbounded;
use notify::*;

mod utils;
use utils::*;

const TEMP_DIR: &str = "temp_dir";

#[test]
fn create_file() {
    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    // macOS FsEvent needs some time to discard old events from its log.
    sleep_macos(10);

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None
            ),]
        );
    } else if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("file1"),
                EventKind::Create(event::CreateKind::Any),
                None
            )]
        );
    } else {
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
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                )
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.write("file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // excessive create event
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ),
            ]
        );
    } else if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("file1"),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            )]
        );
    } else {
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.chmod("file1");

    sleep_macos(10);

    if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("file1"),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            ),]
        );
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Metadata(event::MetadataKind::Any)),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
            ]
        );
        panic!("macos cannot distinguish between chmod and create");
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("file1"),
                EventKind::Modify(event::ModifyKind::Metadata(event::MetadataKind::Any)),
                None
            ),]
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.remove("file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // excessive create event
                (
                    tdir.mkpath("file1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("file1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("file1a", "file1b");

    if cfg!(target_os = "macos") {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("file1a"),
                    EventKind::Create(event::CreateKind::Any),
                    Some(cookies[0])
                ), // excessive create event
                (
                    tdir.mkpath("file1a"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                ),
                (
                    tdir.mkpath("file1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                )
            ]
        );
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
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
                )
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("watch_dir/file1", "file1b");
    tdir.create("watch_dir/file1");

    if cfg!(target_os = "macos") {
        // fsevent interprets a move_out as a rename event
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("watch_dir/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/file1"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ),
            ]
        );
        panic!("move event should be a remove event; fsevent conflates rename and create events");
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("watch_dir/file1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("watch_dir/file1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("file1");
    tdir.write("file1");
    tdir.chmod("file1");

    if cfg!(target_os = "windows") {
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
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ),
            ]
        );
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Metadata(event::MetadataKind::Any)),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ),
            ]
        );
        panic!("macos cannot distinguish between chmod and create");
    } else if cfg!(target_os = "linux") {
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
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ),
                (
                    tdir.mkpath("file1"),
                    EventKind::Modify(event::ModifyKind::Metadata(event::MetadataKind::Any)),
                    None
                ),
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("file1a");
    tdir.rename("file1a", "file1b");

    let actual = recv_events(&rx);

    if cfg!(target_os = "windows") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("file1a"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("file1b"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
                (
                    tdir.mkpath("file1a"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(1)
                ),
                (
                    tdir.mkpath("file1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(1)
                )
            ]
        );
    } else if cfg!(target_os = "macos") {
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
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("file1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ),
            ]
        );
    } else {
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
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
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
                )
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("file1a", "file1b");
    tdir.rename("file1b", "file1c");

    if cfg!(target_os = "macos") {
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
                )
            ]
        );
    } else {
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
                    tdir.mkpath("file1c"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[1])
                )
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1");

    let actual = if cfg!(target_os = "macos") {
        recv_events(&rx)
    } else {
        recv_events(&rx)
    };

    assert_eq!(
        actual,
        vec![(
            tdir.mkpath("dir1"),
            EventKind::Create(event::CreateKind::Any),
            None
        ),]
    );
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1");
    tdir.create("dir1/dir2");

    sleep(100);

    tdir.create("dir1/dir2/file1");

    let actual = if cfg!(target_os = "macos") {
        recv_events(&rx)
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
                    tdir.mkpath("dir1/dir2/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1/dir2/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ),
            ]
        );
    } else if cfg!(target_os = "windows") {
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1/dir2"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1/dir2/file1"),
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
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1/dir2/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.chmod("dir1");

    if cfg!(target_os = "windows") {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("dir1"),
                EventKind::Modify(event::ModifyKind::Data(event::DataChange::Any)),
                None
            ),]
        );
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Modify(event::ModifyKind::Metadata(event::MetadataKind::Any)),
                    None
                ),
                (
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
            ]
        );
        panic!("macos cannot distinguish between chmod and create");
    } else {
        // TODO: emit chmod event only once
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Modify(event::ModifyKind::Metadata(event::MetadataKind::Any)),
                    None
                ),
                (
                    tdir.mkpath("dir1"),
                    EventKind::Modify(event::ModifyKind::Metadata(event::MetadataKind::Any)),
                    None
                ),
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.remove("dir1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ), // excessive create event
                (
                    tdir.mkpath("dir1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![(
                tdir.mkpath("dir1"),
                EventKind::Remove(event::RemoveKind::Any),
                None
            ),]
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1a", "dir1b");

    if cfg!(target_os = "macos") {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
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
            ]
        );
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("watch_dir/dir1", "dir1b");
    tdir.create("watch_dir/dir1");

    if cfg!(target_os = "macos") {
        // fsevent interprets a move_out as a rename event
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("watch_dir/dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ),
            ]
        );
        panic!("move event should be a remove event; fsevent conflates rename and create events");
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("watch_dir/dir1"),
                    EventKind::Remove(event::RemoveKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("watch_dir"), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1", "watch_dir/dir1");

    sleep(100);

    tdir.create("watch_dir/dir1/dir2/file1");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("watch_dir/dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ),
            ]
        );
    } else if cfg!(target_os = "linux") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("watch_dir/dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1/dir2/file1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1/dir2/file1"),
                    EventKind::Access(event::AccessKind::Close(event::AccessMode::Write)),
                    None
                ),
            ]
        );
    } else {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("watch_dir/dir1"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("watch_dir/dir1/dir2/file1"),
                    EventKind::Create(event::CreateKind::Any),
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.create("dir1a");
    tdir.rename("dir1a", "dir1b");

    if cfg!(target_os = "macos") {
        assert_eq!(
            recv_events(&rx),
            vec![
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ),
                (
                    tdir.mkpath("dir1b"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ),
            ]
        );
    } else if cfg!(target_os = "linux") {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1a"),
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
                    tdir.mkpath("dir1b"),
                    EventKind::Modify(event::ModifyKind::Metadata(event::MetadataKind::Any)),
                    None
                ),
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

    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    sleep_windows(100);

    tdir.rename("dir1a", "dir1b");
    tdir.rename("dir1b", "dir1c");

    if cfg!(target_os = "macos") {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            actual,
            vec![
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Create(event::CreateKind::Any),
                    None
                ),
                (
                    tdir.mkpath("dir1a"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    None
                ),
                (
                    tdir.mkpath("dir1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                ),
                (
                    tdir.mkpath("dir1c"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[0])
                ),
            ]
        );
    } else {
        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(
            actual,
            vec![
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
                    tdir.mkpath("dir1b"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[1])
                ),
                (
                    tdir.mkpath("dir1c"),
                    EventKind::Modify(event::ModifyKind::Name(event::RenameMode::Any)),
                    Some(cookies[1])
                ),
            ]
        );
    }
}
