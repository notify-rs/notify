extern crate notify;
extern crate tempdir;
extern crate time;

mod utils;

use notify::*;
use std::sync::mpsc;
use tempdir::TempDir;

use utils::*;

#[test]
fn create_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    // OSX FsEvent needs some time to discard old events from its log.
    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.create("file1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CREATE),
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::CREATE),
        ]);
    }
}

#[test]
fn write_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1"
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.write("file1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CREATE | op::WRITE),
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::WRITE),
        ]);
    }
}

#[test]
#[ignore]
fn modify_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1"
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.chmod("file1");

    if cfg!(target_os="macos") {
        sleep(10);
    }

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::WRITE),
        ]);
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CREATE),
        ]);
        panic!("macos cannot distinguish between chmod and create");
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::CHMOD),
        ]);
    }
}

#[test]
fn delete_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1"
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.remove("file1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CREATE | op::REMOVE),
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::REMOVE),
        ]);
    }
}

#[test]
#[ignore]
fn rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1a"
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.rename("file1a", "file1b");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME),
            (tdir.mkpath("file1b"), op::RENAME)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1a"), op::RENAME),
            (tdir.mkpath("file1b"), op::CREATE)
        ]);
    }
    if cfg!(not(target_os="windows")) {
        panic!("cannot be distinguished from move_out_create_file");
    }
}

#[test]
#[ignore]
fn move_out_create_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "watch_dir/file1"
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.rename("watch_dir/file1", "file1b");
    tdir.create("watch_dir/file1");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("watch_dir/file1"), op::REMOVE), // windows interprets a move out of the watched directory as a remove
            (tdir.mkpath("watch_dir/file1"), op::CREATE)
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("watch_dir/file1"), op::CREATE | op::RENAME),
            (tdir.mkpath("watch_dir/file1"), op::CREATE)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("watch_dir/file1"), op::RENAME),
            (tdir.mkpath("watch_dir/file1"), op::CREATE)
        ]);
    }
    if cfg!(not(target_os="windows")) {
        panic!("cannot be distinguished from rename_file");
    }
}

#[test]
#[ignore]
fn create_write_modify_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.create("file1");
    tdir.write("file1");
    tdir.chmod("file1");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::CREATE),
            (tdir.mkpath("file1"), op::WRITE),
            (tdir.mkpath("file1"), op::WRITE),
        ]);
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1"), op::CREATE | op::WRITE),
        ]);
        panic!("macos cannot distinguish between chmod and create");
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1"), op::CREATE),
            (tdir.mkpath("file1"), op::WRITE),
            (tdir.mkpath("file1"), op::CHMOD),
        ]);
    }
}

#[test]
fn create_rename_overwrite_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1b",
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.create("file1a");
    tdir.rename("file1a", "file1b");

    if cfg!(target_os="windows") {
        // Windows interprets a move that overwrites a file as a delete of the source file and a write to the file that is being overwritten
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1a"), op::CREATE),
            (tdir.mkpath("file1a"), op::REMOVE),
            (tdir.mkpath("file1b"), op::WRITE)
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME),
            (tdir.mkpath("file1b"), op::CREATE | op::RENAME)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1a"), op::CREATE),
            (tdir.mkpath("file1a"), op::RENAME),
            (tdir.mkpath("file1b"), op::CREATE)
        ]);
    }
}

#[test]
#[ignore]
fn rename_rename_file() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "file1a",
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.rename("file1a", "file1b");
    tdir.rename("file1b", "file1c");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME),
            (tdir.mkpath("file1b"), op::RENAME),
            (tdir.mkpath("file1c"), op::RENAME)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("file1a"), op::RENAME),
            (tdir.mkpath("file1b"), op::CREATE),
            (tdir.mkpath("file1b"), op::RENAME),
            (tdir.mkpath("file1c"), op::CREATE)
        ]);
    }
}

#[test]
fn create_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.create("dir1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("dir1"), op::CREATE),
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1"), op::CREATE),
        ]);
    }
}

#[test]
#[ignore]
fn modify_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.chmod("dir1");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1"), op::WRITE),
        ]);
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("dir1"), op::CREATE),
        ]);
        panic!("macos cannot distinguish between chmod and create");
    } else {
        // TODO: emit chmod event only once
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1"), op::CHMOD),
            (tdir.mkpath("dir1"), op::CHMOD),
        ]);
    }
}

#[test]
#[ignore]
fn delete_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1",
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.remove("dir1");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("dir1"), op::CREATE | op::REMOVE),
        ]);
    } else if cfg!(target_os="linux") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1"), op::IGNORED),
            (tdir.mkpath("dir1"), op::REMOVE),
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1").clone(), op::REMOVE),
        ]);
    }
}

#[test]
#[ignore]
fn rename_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1a",
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.rename("dir1a", "dir1b");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("dir1a"), op::CREATE | op::RENAME),
            (tdir.mkpath("dir1b"), op::RENAME),
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1a"), op::RENAME),
            (tdir.mkpath("dir1b"), op::CREATE),
        ]);
    }
    if cfg!(not(target_os="windows")) {
        panic!("cannot be distinguished from move_out_create_directory");
    }
}

#[test]
#[ignore]
fn move_out_create_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "watch_dir/dir1",
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("watch_dir"), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.rename("watch_dir/dir1", "dir1b");
    tdir.create("watch_dir/dir1");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("watch_dir/dir1"), op::REMOVE), // windows interprets a move out of the watched directory as a remove
            (tdir.mkpath("watch_dir/dir1"), op::CREATE),
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("watch_dir/dir1"), op::CREATE | op::RENAME),
            (tdir.mkpath("watch_dir/dir1"), op::CREATE),
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("watch_dir/dir1"), op::RENAME),
            (tdir.mkpath("watch_dir/dir1"), op::CREATE),
        ]);
    }
    if cfg!(not(target_os="windows")) {
        panic!("cannot be distinguished from rename_directory");
    }
}

#[test]
#[ignore]
fn create_rename_overwrite_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1b",
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.create("dir1a");
    tdir.rename("dir1a", "dir1b");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1a"), op::CREATE),
            (tdir.mkpath("dir1b"), op::RENAME),
            (tdir.mkpath("dir1b"), op::CREATE),
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("dir1a"), op::CREATE | op::RENAME),
            (tdir.mkpath("dir1b"), op::CREATE | op::RENAME),
        ]);
    } else if cfg!(target_os="linux") {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1a"), op::CREATE),
            (tdir.mkpath("dir1a"), op::RENAME),
            (tdir.mkpath("dir1b"), op::CREATE),
            (tdir.mkpath("dir1b"), op::CHMOD),
            (tdir.mkpath("dir1b"), op::IGNORED),
        ]);
    } else {
        unimplemented!();
    }
}

#[test]
#[ignore]
fn rename_rename_directory() {
    let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

    tdir.create_all(vec![
        "dir1a",
    ]);

    if cfg!(target_os="macos") {
        sleep(10);
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        sleep(100);
    }

    tdir.rename("dir1a", "dir1b");
    tdir.rename("dir1b", "dir1c");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (tdir.mkpath("dir1a"), op::CREATE | op::RENAME),
            (tdir.mkpath("dir1b"), op::RENAME),
            (tdir.mkpath("dir1c"), op::RENAME),
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (tdir.mkpath("dir1a"), op::RENAME),
            (tdir.mkpath("dir1b"), op::CREATE),
            (tdir.mkpath("dir1b"), op::RENAME),
            (tdir.mkpath("dir1c"), op::CREATE),
        ]);
    }
}
