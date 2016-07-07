extern crate notify;
extern crate tempdir;
extern crate tempfile;
extern crate time;

mod utils;

use notify::*;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc::{self, channel, Sender};
use tempdir::TempDir;
use tempfile::NamedTempFile;
use std::fs;
use std::thread;
use std::time::Duration;

use utils::*;

#[cfg(not(target_os="windows"))]
use std::os::unix::fs::PermissionsExt;

#[cfg(not(target_os="windows"))]
fn chmod(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(777))
}

#[cfg(target_os="windows")]
fn chmod(path: &Path) -> io::Result<()> {
    let mut permissions = try!(fs::metadata(path)).permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)
}

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
        thread::sleep(Duration::from_millis(10));
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
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path = canonicalize(temp_dir.path());
    path.push("new_file.bin");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::File::create(path.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![(path, op::CREATE)]);
    } else {
        assert_eq!(recv_events(&rx), vec![(path, op::CREATE)]);
    }
}

#[test]
fn write_file() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path = canonicalize(temp_dir.path());
    path.push("file.bin");

    let mut file = fs::File::create(path.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    file.write("some data".as_bytes()).expect("failed to write to file");
    file.sync_all().expect("failed to sync file");
    if cfg!(target_os="macos") {
        drop(file); // file needs to be closed for fsevent
    }

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![(path, op::CREATE | op::WRITE)]);
    } else {
        assert_eq!(recv_events(&rx), vec![(path, op::WRITE)]);
    }
}

#[test]
#[ignore]
fn modify_file() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path = canonicalize(temp_dir.path());
    path.push("file.bin");

    fs::File::create(path.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    chmod(path.as_path()).expect("failed to chmod file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![(path, op::WRITE)]);
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![(path, op::CREATE)]);
        panic!("macos cannot distinguish between chmod and create");
    } else {
        assert_eq!(recv_events(&rx), vec![(path, op::CHMOD)]);
    }
}

#[test]
fn delete_file() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path = canonicalize(temp_dir.path());
    path.push("file.bin");

    fs::File::create(path.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::remove_file(path.as_path()).expect("failed to remove file");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![(path, op::CREATE | op::REMOVE)]);
    } else {
        assert_eq!(recv_events(&rx), vec![(path, op::REMOVE)]);
    }
}

#[test]
#[ignore]
fn rename_file() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path1 = canonicalize(temp_dir.path());
    path1.push("file1.bin");
    let mut path2 = canonicalize(temp_dir.path());
    path2.push("file2.bin");

    fs::File::create(path1.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::rename(path1.as_path(), path2.as_path()).expect("failed to rename file");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path1, op::CREATE | op::RENAME),
            (path2, op::RENAME)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path1, op::RENAME),
            (path2, op::CREATE)
        ]);
    }
    if cfg!(not(target_os="windows")) {
        panic!("cannot be distinguished from move_out_create_file");
    }
}

#[test]
#[ignore]
fn move_out_create_file() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let sub_dir1 = TempDir::new_in(temp_dir.path(), "sub_dir1").expect("failed to create temporary directory");
    let sub_dir2 = TempDir::new_in(temp_dir.path(), "sub_dir2").expect("failed to create temporary directory");
    let mut path1a = canonicalize(sub_dir1.path());
    path1a.push("file1.bin");
    let mut path1b = canonicalize(sub_dir2.path());
    path1b.push("file1.bin");
    let mut path2 = canonicalize(sub_dir1.path());
    path2.push("file2.bin");

    fs::File::create(path1a.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(sub_dir1.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::rename(path1a.as_path(), path1b.as_path()).expect("failed to rename file");
    fs::File::create(path2.as_path()).expect("failed to create file");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (path1a, op::REMOVE), // windows interprets a move out of the watched directory as a remove
            (path2, op::CREATE)
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path1a, op::CREATE | op::RENAME),
            (path2, op::CREATE)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path1a, op::RENAME),
            (path2, op::CREATE)
        ]);
    }
    if cfg!(not(target_os="windows")) {
        panic!("cannot be distinguished from rename_file");
    }
}

#[test]
#[ignore]
fn create_write_modify_file() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path = canonicalize(temp_dir.path());
    path.push("new_file.bin");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    let mut file = fs::File::create(path.as_path()).expect("failed to create file");
    file.write("some data".as_bytes()).expect("failed to write to file");
    if cfg!(target_os="macos") {
        drop(file); // file needs to be closed for fsevent
    }
    chmod(path.as_path()).expect("failed to chmod file");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (path.clone(), op::CREATE),
            (path.clone(), op::WRITE),
            (path, op::WRITE)
        ]);
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path.clone(), op::CREATE | op::WRITE)
        ]);
        panic!("macos cannot distinguish between chmod and create");
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path.clone(), op::CREATE),
            (path.clone(), op::WRITE),
            (path, op::CHMOD)
        ]);
    }
}

#[test]
fn create_rename_overwrite_file() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path1 = canonicalize(temp_dir.path());
    path1.push("file1.bin");
    let mut path2 = canonicalize(temp_dir.path());
    path2.push("file2.bin");

    fs::File::create(path2.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::File::create(path1.as_path()).expect("failed to create file");
    fs::rename(path1.as_path(), path2.as_path()).expect("failed to rename file");

    if cfg!(target_os="windows") {
        // Windows interprets a move that overwrites a file as a delete of the source file and a write to the file that is being overwritten
        assert_eq!(recv_events(&rx), vec![
            (path1.clone(), op::CREATE),
            (path1, op::REMOVE),
            (path2, op::WRITE)
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path1, op::CREATE | op::RENAME),
            (path2, op::CREATE | op::RENAME)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path1.clone(), op::CREATE),
            (path1, op::RENAME),
            (path2, op::CREATE)
        ]);
    }
}

#[test]
#[ignore]
fn rename_rename_file() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path1 = canonicalize(temp_dir.path());
    path1.push("file1.bin");
    let mut path2 = canonicalize(temp_dir.path());
    path2.push("file2.bin");
    let mut path3 = canonicalize(temp_dir.path());
    path3.push("file3.bin");

    fs::File::create(path1.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::rename(path1.as_path(), path2.as_path()).expect("failed to rename file");
    fs::rename(path2.as_path(), path3.as_path()).expect("failed to rename file");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path1, op::CREATE | op::RENAME),
            (path2, op::RENAME),
            (path3, op::RENAME)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path1, op::RENAME),
            (path2.clone(), op::CREATE),
            (path2, op::RENAME),
            (path3, op::CREATE)
        ]);
    }
}

#[test]
fn create_directory() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path = canonicalize(temp_dir.path());
    path.push("new_dir");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::create_dir(path.as_path()).expect("failed to create directory");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![(path, op::CREATE)]);
    } else {
        assert_eq!(recv_events(&rx), vec![(path, op::CREATE)]);
    }
}

#[test]
#[ignore]
fn modify_directory() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path = canonicalize(temp_dir.path());
    path.push("dir");

    fs::create_dir(path.as_path()).expect("failed to create directory");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    chmod(path.as_path()).expect("failed to chmod directory");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![(path, op::WRITE)]);
        panic!("windows cannot distinguish between chmod and write");
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![(path, op::CREATE)]);
        panic!("macos cannot distinguish between chmod and create");
    } else {
        // TODO: emit chmod event only once
        assert_eq!(recv_events(&rx), vec![
            (path.clone(), op::CHMOD),
            (path, op::CHMOD)
        ]);
    }
}

#[test]
#[ignore]
fn delete_directory() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path = canonicalize(temp_dir.path());
    path.push("dir");

    fs::create_dir(path.as_path()).expect("failed to create directory");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::remove_dir(path.as_path()).expect("failed to remove directory");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path, op::CREATE | op::REMOVE)
        ]);
    } else if cfg!(target_os="linux") {
        assert_eq!(recv_events(&rx), vec![
            (path.clone(), op::IGNORED),
            (path, op::REMOVE)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path.clone(), op::REMOVE)
        ]);
    }
}

#[test]
#[ignore]
fn rename_directory() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path1 = canonicalize(temp_dir.path());
    path1.push("dir1");
    let mut path2 = canonicalize(temp_dir.path());
    path2.push("dir2");

    fs::create_dir(path1.as_path()).expect("failed to create directory");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::rename(path1.as_path(), path2.as_path()).expect("failed to rename directory");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path1, op::CREATE | op::RENAME),
            (path2, op::RENAME)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path1, op::RENAME),
            (path2, op::CREATE)
        ]);
    }
    if cfg!(not(target_os="windows")) {
        panic!("cannot be distinguished from move_out_create_directory");
    }
}

#[test]
#[ignore]
fn move_out_create_directory() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let sub_dir1 = TempDir::new_in(temp_dir.path(), "sub_dir1").expect("failed to create temporary directory");
    let sub_dir2 = TempDir::new_in(temp_dir.path(), "sub_dir2").expect("failed to create temporary directory");
    let mut path1a = canonicalize(sub_dir1.path());
    path1a.push("dir1.bin");
    let mut path1b = canonicalize(sub_dir2.path());
    path1b.push("dir1.bin");
    let mut path2 = canonicalize(sub_dir1.path());
    path2.push("dir2.bin");

    fs::File::create(path1a.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(sub_dir1.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::rename(path1a.as_path(), path1b.as_path()).expect("failed to rename file");
    fs::File::create(path2.as_path()).expect("failed to create file");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (path1a, op::REMOVE), // windows interprets a move out of the watched directory as a remove
            (path2, op::CREATE)
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path1a, op::CREATE | op::RENAME),
            (path2, op::CREATE)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path1a, op::RENAME),
            (path2, op::CREATE)
        ]);
    }
    if cfg!(not(target_os="windows")) {
        panic!("cannot be distinguished from rename_directory");
    }
}

#[test]
#[ignore]
fn create_rename_overwrite_directory() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path1 = canonicalize(temp_dir.path());
    path1.push("dir1");
    let mut path2 = canonicalize(temp_dir.path());
    path2.push("dir2");

    fs::create_dir(path2.as_path()).expect("failed to create directory");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::create_dir(path1.as_path()).expect("failed to create directory");
    fs::rename(path1.as_path(), path2.as_path()).expect("failed to rename directory");

    if cfg!(target_os="windows") {
        assert_eq!(recv_events(&rx), vec![
            (path1.clone(), op::CREATE),
            (path1, op::RENAME),
            (path2, op::CREATE)
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path1, op::CREATE | op::RENAME),
            (path2, op::CREATE | op::RENAME)
        ]);
    } else if cfg!(target_os="linux") {
        assert_eq!(recv_events(&rx), vec![
            (path1.clone(), op::CREATE),
            (path1, op::RENAME),
            (path2.clone(), op::CREATE),
            (path2.clone(), op::CHMOD),
            (path2, op::IGNORED),
        ]);
    } else {
        unimplemented!();
    }
}

#[test]
#[ignore]
fn rename_rename_directory() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path1 = canonicalize(temp_dir.path());
    path1.push("dir1");
    let mut path2 = canonicalize(temp_dir.path());
    path2.push("dir2");
    let mut path3 = canonicalize(temp_dir.path());
    path3.push("dir3");

    fs::create_dir(path1.as_path()).expect("failed to create directory");

    if cfg!(target_os="macos") {
    	thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::rename(path1.as_path(), path2.as_path()).expect("failed to rename directory");
    fs::rename(path2.as_path(), path3.as_path()).expect("failed to rename directory");

    if cfg!(target_os="macos") {
        assert_eq!(inflate_events(recv_events(&rx)), vec![
            (path1, op::CREATE | op::RENAME),
            (path2, op::RENAME),
            (path3, op::RENAME)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path1, op::RENAME),
            (path2.clone(), op::CREATE),
            (path2, op::RENAME),
            (path3, op::CREATE)
        ]);
    }
}
