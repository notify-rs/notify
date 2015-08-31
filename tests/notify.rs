extern crate notify;
extern crate tempdir;
extern crate tempfile;
extern crate time;


use notify::*;
use std::io::Write;
#[cfg(target_os="macos")]
use std::path::Component;
#[cfg(target_os="macos")]
use std::fs::read_link;
use std::path::{Path, PathBuf};
use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver};
use tempdir::TempDir;
use tempfile::NamedTempFile;

const TIMEOUT_S: f64 = 5.0;

// TODO: replace with std::fs::canonicalize rust-lang/rust#27706.
// OSX needs to resolve symlinks
#[cfg(target_os="macos")]
fn resolve_path(path: &Path) -> PathBuf {
    let p = path.to_str().unwrap();

    let mut out = PathBuf::new();
    let buf = PathBuf::from(p);
    for p in buf.components() {
        match p {
          Component::RootDir => out.push("/"),
          Component::Normal(osstr) => {
              out.push(osstr);
              if let Ok(real) = read_link(&out) {
                  if real.is_relative() {
                    out.pop();
                    out.push(real);
                  } else {
                    out = real;
                  }
              }
          }
          _ => ()
        }
    }
    out
}


#[cfg(not(any(target_os="macos")))]
fn resolve_path(path: &Path) -> PathBuf {
    let p = path.to_str().unwrap();
    PathBuf::from(p)
}

fn validate_recv(rx: Receiver<Event>, evs: Vec<(&Path, Op)>) {
  let deadline = time::precise_time_s() + TIMEOUT_S;
  let mut evs = evs.clone();

  while time::precise_time_s() < deadline {
    if let Ok(actual) = rx.try_recv() {
      let path = actual.path.clone().unwrap();
      let op = actual.op.unwrap().clone();
      let mut removables = vec!();
      for i in (0..evs.len()) {
        let expected = evs.get(i).unwrap();
        if path.clone().as_path() == expected.0 && op.contains(expected.1) {
            removables.push(i);
        }
      }
      for removable in removables {
        evs.remove(removable);
      }
    }
    if evs.is_empty() { break; }
  }
  assert!(evs.is_empty(),
    "Some expected events did not occur before the test timedout:\n\t\t{:?}", evs);
}

fn validate_watch_single_file<F, W>(ctor: F) where
  F: Fn(Sender<Event>) -> Result<W, Error>, W: Watcher {
  let mut file = NamedTempFile::new().unwrap();
  let (tx, rx) = channel();
  let mut w = ctor(tx).unwrap();
  w.watch(file.path()).unwrap();
  thread::sleep_ms(1000);
  file.write_all(b"foo").unwrap();
  file.flush().unwrap();
  validate_recv(rx, vec![(resolve_path(file.path()).as_path(), op::CREATE)]);
}


fn validate_watch_dir<F, W>(ctor: F) where
  F: Fn(Sender<Event>) -> Result<W, Error>, W: Watcher {
  let dir = TempDir::new("dir").unwrap();
  let dir1 = TempDir::new_in(dir.path(), "dir1").unwrap();
  let dir2 = TempDir::new_in(dir.path(), "dir2").unwrap();
  let dir11 = TempDir::new_in(dir1.path(), "dir11").unwrap();
  let (tx, rx) = channel();
  // OSX FsEvent needs some time to discard old events from its log.
  thread::sleep_ms(12000);
  let mut w = ctor(tx).unwrap();

  w.watch(dir.path()).unwrap();
  let f111 = NamedTempFile::new_in(dir11.path()).unwrap();
  let f111_path = f111.path().to_owned();
  let f111_path = f111_path.as_path();
  let f21 = NamedTempFile::new_in(dir2.path()).unwrap();
  thread::sleep_ms(4000);
  f111.close().unwrap();
  thread::sleep_ms(4000);
  validate_recv(rx, vec![(resolve_path(f111_path).as_path(), op::CREATE),
                         (resolve_path(f21.path()).as_path(), op::CREATE),
                         (resolve_path(f111_path).as_path(), op::REMOVE)]);
}

#[test]
fn watch_single_file_recommended() {
  validate_watch_single_file(RecommendedWatcher::new);
}

#[test]
fn watch_dir_recommended() {
  validate_watch_dir(RecommendedWatcher::new);
}

#[test]
fn watch_single_file_poll() {
  // Currently broken on OSX because relative filename are sent.
  validate_watch_single_file(PollWatcher::new);
}

#[cfg(target_os = "linux")]
#[test]
fn new_inotify() {
  let (tx, _) = channel();
  let w: Result<INotifyWatcher, Error> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[cfg(target_os = "macos")]
#[test]
fn new_fsevent() {
  let (tx, _) = channel();
  let w: Result<FsEventWatcher, Error> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[test]
fn new_null() {
  let (tx, _) = channel();
  let w: Result<NullWatcher, Error> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[test]
fn new_poll() {
  let (tx, _) = channel();
  let w: Result<PollWatcher, Error> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[test]
fn new_recommended() {
  let (tx, _) = channel();
  let w: Result<RecommendedWatcher, Error> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}
