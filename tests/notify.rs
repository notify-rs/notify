extern crate notify;
extern crate tempdir;
extern crate tempfile;
extern crate time;

mod utils;

use notify::*;
use std::io::Write;
use std::sync::mpsc::{self, channel, Sender};
use tempdir::TempDir;
use tempfile::NamedTempFile;

use utils::*;

fn validate_watch_single_file<F, W>(ctor: F) where
    F: Fn(Sender<Event>) -> Result<W>, W: Watcher
{
    let (tx, rx) = channel();
    let mut w = ctor(tx).unwrap();

    // While the file is open, windows won't report modified events for it.
    // Flushing doesn't help.  So make sure it is closed before we validate.
    let path = {
        let mut file = NamedTempFile::new().unwrap();
        w.watch(file.path(), RecursiveMode::Recursive).unwrap();

        // make some files that should be exlcuded from watch. this works because tempfile creates
        // them all in the same directory.
        let mut excluded_file = NamedTempFile::new().unwrap();
        let another_excluded_file = NamedTempFile::new().unwrap();
        let _ = another_excluded_file; // eliminate warning
        excluded_file.write_all(b"shouldn't get an event for this").expect("failed to write to file");
        excluded_file.sync_all().expect("failed to sync file");

        file.write_all(b"foo").expect("failed to write to file");
        file.sync_all().expect("failed to sync file");
        canonicalize(file.path())
    };

    // make sure that ONLY the target path is in the list of received events

    if cfg!(target_os="windows") {
        // Windows does not support chmod
        assert_eq!(recv_events(&rx), vec![
            (path.clone(), op::WRITE),
            (path, op::REMOVE)
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path, op::CREATE | op::REMOVE | op::WRITE)
        ]);
    } else if cfg!(target_os="linux") {
        // Only linux supports ignored
        assert_eq!(recv_events(&rx), vec![
            (path.clone(), op::WRITE),
            (path.clone(), op::CHMOD),
            (path.clone(), op::REMOVE),
            (path, op::IGNORED)
        ]);
    } else {
        unimplemented!();
    }
}

fn validate_watch_dir<F, W>(ctor: F) where
    F: Fn(Sender<Event>) -> Result<W>, W: Watcher
{
    let dir = TempDir::new("dir").unwrap();
    let dir1 = TempDir::new_in(dir.path(), "dir1").unwrap();
    let dir2 = TempDir::new_in(dir.path(), "dir2").unwrap();
    let dir11 = TempDir::new_in(dir1.path(), "dir11").unwrap();
    let (tx, rx) = channel();
    let mut w = ctor(tx).unwrap();

    // OSX FsEvent needs some time to discard old events from its log.
    if cfg!(target_os="macos") {
        sleep(10);
    }

    w.watch(dir.path(), RecursiveMode::Recursive).unwrap();

    let f111 = NamedTempFile::new_in(dir11.path()).unwrap();
    let f111_path = canonicalize(f111.path());
    let f21 = NamedTempFile::new_in(dir2.path()).unwrap();
    let f21_path = canonicalize(f21.path());
    f111.close().unwrap();

    if cfg!(target_os="windows") {
        // Windows may sneak a write event in there
        let mut actual = recv_events(&rx);
        actual.retain(|&(_, op)| op != op::WRITE);
        assert_eq!(actual, vec![
            (f111_path.clone(), op::CREATE),
            (f21_path, op::CREATE),
            (f111_path, op::REMOVE)
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (f21_path, op::CREATE),
            (f111_path, op::CREATE | op::REMOVE)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (f111_path.clone(), op::CREATE),
            (f21_path, op::CREATE),
            (f111_path, op::REMOVE)
        ]);
    }
}

#[test]
fn watch_single_file_recommended() {
    validate_watch_single_file(RecommendedWatcher::new);
}

#[test]
fn watch_dir_recommended() {
    validate_watch_dir(RecommendedWatcher::new);
}

// Currently broken on OSX because relative filename are sent.
// Also panics on random Linux test passes.
#[cfg(broken)]
#[test]
fn watch_single_file_poll() {
    validate_watch_single_file(PollWatcher::new);
}

#[test]
fn create_file() {
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
