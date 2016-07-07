extern crate notify;
extern crate tempdir;
extern crate tempfile;
extern crate time;

mod utils;

use notify::*;
use std::io::Write;
use std::sync::mpsc;
use tempdir::TempDir;
use std::fs;
use std::thread;
use std::time::Duration;

use utils::*;

#[cfg(target_os="linux")]
#[test]
fn new_inotify() {
    let (tx, _) = mpsc::channel();
    let w: Result<INotifyWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[cfg(target_os="macos")]
#[test]
fn new_fsevent() {
    let (tx, _) = mpsc::channel();
    let w: Result<FsEventWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[test]
fn new_null() {
    let (tx, _) = mpsc::channel();
    let w: Result<NullWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[test]
fn new_poll() {
    let (tx, _) = mpsc::channel();
    let w: Result<PollWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

#[test]
fn new_recommended() {
    let (tx, _) = mpsc::channel();
    let w: Result<RecommendedWatcher> = Watcher::new(tx);
    assert!(w.is_ok());
}

// if this test builds, it means RecommendedWatcher is Send.
#[test]
fn test_watcher_send() {
    let (tx, _) = mpsc::channel();

    let mut watcher: RecommendedWatcher = Watcher::new(tx).unwrap();

    thread::spawn(move || {
        watcher.watch(".", RecursiveMode::Recursive).unwrap();
    }).join().unwrap();
}

// if this test builds, it means RecommendedWatcher is Sync.
#[test]
fn test_watcher_sync() {
    use std::sync::{ Arc, RwLock };

    let (tx, _) = mpsc::channel();

    let watcher: RecommendedWatcher = Watcher::new(tx).unwrap();
    let watcher = Arc::new(RwLock::new(watcher));

    thread::spawn(move || {
        let mut watcher = watcher.write().unwrap();
        watcher.watch(".", RecursiveMode::Recursive).unwrap();
    }).join().unwrap();
}

#[test]
fn watch_recursive_create_directory() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut new_dir = canonicalize(temp_dir.path());
    new_dir.push("new_dir");
    let mut file1 = new_dir.clone();
    file1.push("file1");
    let mut file2 = new_dir.clone();
    file2.push("file2");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.path().to_owned(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::create_dir(new_dir.as_path()).expect("failed to create directory");
    thread::sleep(Duration::from_millis(10));
    fs::File::create(file1.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(100));
    }

    watcher.unwatch(temp_dir.into_path()).expect("failed to unwatch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::File::create(file2.as_path()).expect("failed to create file");

    assert_eq!(recv_events(&rx), vec![
        (new_dir, op::CREATE),
        (file1, op::CREATE)
    ]);
}

#[test]
#[ignore]
fn watch_recursive_move() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut sub_dir1a = canonicalize(temp_dir.path());
    sub_dir1a.push("sub_dir1a");
    let mut sub_dir1b = canonicalize(temp_dir.path());
    sub_dir1b.push("sub_dir1b");
    let mut path1 = canonicalize(temp_dir.path());
    path1.push("sub_dir1a");
    path1.push("file1.bin");
    let mut path2 = canonicalize(temp_dir.path());
    path2.push("sub_dir1b");
    path2.push("file2.bin");

    fs::create_dir(sub_dir1a.as_path()).expect("failed to create directory");

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
    fs::rename(sub_dir1a.as_path(), sub_dir1b.as_path()).expect("failed to rename file");
    thread::sleep(Duration::from_millis(10));
    fs::File::create(path2.as_path()).expect("failed to create file");

    let actual = if cfg!(target_os="windows") {
        // Windows may sneak a write event in there
        let mut events = recv_events(&rx);
        events.retain(|&(_, op)| op != op::WRITE);
        events
    } else if cfg!(target_os="macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os="windows") {
        assert_eq!(actual, vec![
            (path1, op::CREATE),
            (sub_dir1a, op::RENAME),
            (sub_dir1b, op::RENAME), // should be a create
            (path2, op::CREATE)
        ]);
        panic!("move_to should be translated to create");
    } else if cfg!(target_os="macos") {
        assert_eq!(actual, vec![
            (path1, op::CREATE),
            (sub_dir1a, op::CREATE | op::RENAME), // excessive create event
            (sub_dir1b, op::RENAME), // should be a create
            (path2, op::CREATE)
        ]);
    } else {
        assert_eq!(actual, vec![
            (path1, op::CREATE),
            (sub_dir1a, op::RENAME),
            (sub_dir1b, op::CREATE),
            (path2, op::CREATE)
        ]);
    }
}

#[test]
#[ignore]
fn watch_recursive_move_in() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut sub_dir1 = canonicalize(temp_dir.path());
    sub_dir1.push("sub_dir1");
    let mut sub_dir2 = sub_dir1.clone();
    sub_dir2.push("sub_dir2");
    let watch_dir = TempDir::new_in(temp_dir.path(), "watch_dir").expect("failed to create temporary directory");
    let mut sub_dir1a = canonicalize(watch_dir.path());
    sub_dir1a.push("sub_dir1");
    let mut path = canonicalize(watch_dir.path());
    path.push("sub_dir1");
    path.push("sub_dir2");
    path.push("new_file.bin");

    fs::create_dir(sub_dir1.as_path()).expect("failed to create directory");
    fs::create_dir(sub_dir2.as_path()).expect("failed to create directory");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(watch_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::rename(sub_dir1.as_path(), sub_dir1a.as_path()).expect("failed to rename file");
    thread::sleep(Duration::from_millis(10));
    fs::File::create(path.as_path()).expect("failed to create file");

    let actual = if cfg!(target_os="windows") {
        // Windows may sneak a write event in there
        let mut events = recv_events(&rx);
        events.retain(|&(_, op)| op != op::WRITE);
        events
    } else if cfg!(target_os="macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os="macos") {
        assert_eq!(actual, vec![
            (sub_dir1a, op::RENAME), // fsevents interprets a move_to as a rename event
            (path, op::CREATE)
        ]);
        panic!("move event should be a create event");
    } else {
        assert_eq!(actual, vec![
            (sub_dir1a, op::CREATE),
            (path, op::CREATE)
        ]);
    }
}

#[test]
fn watch_recursive_move_out() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let watch_dir = TempDir::new_in(temp_dir.path(), "watch_dir").expect("failed to create temporary directory");

    let mut sub_dir1a = canonicalize(watch_dir.path());
    sub_dir1a.push("sub_dir1a");
    let mut sub_dir1b = canonicalize(temp_dir.path());
    sub_dir1b.push("sub_dir1b");
    let mut sub_dir2 = sub_dir1a.clone();
    sub_dir2.push("sub_dir2");
    let mut path1 = canonicalize(watch_dir.path());
    path1.push("sub_dir1a");
    path1.push("sub_dir2");
    path1.push("file1.bin");
    let mut path2 = canonicalize(temp_dir.path());
    path2.push("sub_dir1b");
    path2.push("sub_dir2");
    path2.push("file2.bin");

    fs::create_dir(sub_dir1a.as_path()).expect("failed to create directory");
    fs::create_dir(sub_dir2.as_path()).expect("failed to create directory");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(watch_dir.into_path(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::File::create(path1.as_path()).expect("failed to create file");
    fs::rename(sub_dir1a.as_path(), sub_dir1b.as_path()).expect("failed to rename file");
    thread::sleep(Duration::from_millis(10));
    fs::File::create(path2.as_path()).expect("failed to create file");

    let actual = if cfg!(target_os="windows") {
        // Windows may sneak a write event in there
        let mut events = recv_events(&rx);
        events.retain(|&(_, op)| op != op::WRITE);
        events
    } else if cfg!(target_os="macos") {
        inflate_events(recv_events(&rx))
    } else {
        recv_events(&rx)
    };

    if cfg!(target_os="windows") {
        assert_eq!(actual, vec![
            (path1, op::CREATE),
            (sub_dir1a, op::REMOVE) // windows interprets a move out of the watched directory as a remove
        ]);
    } else if cfg!(target_os="macos") {
        assert_eq!(actual, vec![
            (path1, op::CREATE),
            (sub_dir1a, op::CREATE | op::RENAME) // excessive create event
        ]);
    } else {
        assert_eq!(actual, vec![
            (path1, op::CREATE),
            (sub_dir1a, op::RENAME)
        ]);
    }
}

#[test]
fn watch_nonrecursive() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut sub_dir1 = canonicalize(temp_dir.path());
    sub_dir1.push("sub_dir1");
    let mut sub_dir2 = canonicalize(temp_dir.path());
    sub_dir2.push("sub_dir2");
    let mut file0 = canonicalize(temp_dir.path());
    file0.push("file0.bin");
    let mut file1 = sub_dir1.clone();
    file1.push("file1.bin");
    let mut file2 = sub_dir2.clone();
    file2.push("file2.bin");

    fs::create_dir(sub_dir1.as_path()).expect("failed to create directory");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(temp_dir.into_path(), RecursiveMode::NonRecursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    fs::create_dir(sub_dir2.as_path()).expect("failed to create directory");
    thread::sleep(Duration::from_millis(10));
    fs::File::create(file0.as_path()).expect("failed to create file");
    fs::File::create(file1.as_path()).expect("failed to create file");
    fs::File::create(file2.as_path()).expect("failed to create file");

    assert_eq!(recv_events(&rx), vec![
        (sub_dir2, op::CREATE),
        (file0, op::CREATE)
    ]);
}

#[test]
fn watch_file() {
    let temp_dir = TempDir::new("temp_dir").expect("failed to create temporary directory");
    let mut path1 = canonicalize(temp_dir.path());
    path1.push("file1.bin");
    let mut path2 = canonicalize(temp_dir.path());
    path2.push("file2.bin");

    let mut file1 = fs::File::create(path1.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        thread::sleep(Duration::from_millis(10));
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx).expect("failed to create recommended watcher");
    watcher.watch(path1.clone(), RecursiveMode::Recursive).expect("failed to watch directory");

    if cfg!(target_os="windows") {
        thread::sleep(Duration::from_millis(100));
    }

    file1.write("some data".as_bytes()).expect("failed to write to file");
    file1.sync_all().expect("failed to sync file");
    if cfg!(target_os="macos") {
        drop(file1); // file needs to be closed for fsevent
    }

    fs::File::create(path2.as_path()).expect("failed to create file");

    if cfg!(target_os="macos") {
        assert_eq!(recv_events(&rx), vec![
            (path1, op::CREATE | op::WRITE)
        ]);
    } else {
        assert_eq!(recv_events(&rx), vec![
            (path1, op::WRITE)
        ]);
    }
}
