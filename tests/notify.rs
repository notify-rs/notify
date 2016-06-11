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
use std::time::Duration;
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};
#[cfg(not(target_os="linux"))]
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

fn validate_recv(rx: Receiver<Event>, evs: Vec<(&Path, Op)>) -> Vec<Event> {
  let deadline = time::precise_time_s() + TIMEOUT_S;
  let mut evs = evs.clone();

  let mut received_events:Vec<Event> = Vec::new();

  while time::precise_time_s() < deadline {
    match rx.try_recv() {
      Ok(Event{path: Some(path), op: Ok(op)}) => {
        evs.retain(|event| {
          !(path.as_path() == event.0 && op.contains(event.1))
        });
        received_events.push(Event{path: Some(path), op: Ok(op)});
      },
      Ok(Event{path: None, ..})  => (),
      Ok(Event{op: Err(e), ..}) => panic!("unexpected channel err: {:?}", e),
      Err(TryRecvError::Empty) => (),
      Err(e) => panic!("unexpected channel err: {:?}", e)
    }
    if evs.is_empty() { break; }
  }
  assert!(evs.is_empty(),
    "Some expected events did not occur before the test timedout:\n\t\t{:?}", evs);

  received_events
}

#[cfg(target_os = "windows")]
// Windows needs to test this differently since it can't watch files that don't exist yet.
fn validate_watch_single_file<F, W>(ctor: F) where
  F: Fn(Sender<Event>) -> Result<W>, W: Watcher {

  let (tx, rx) = channel();
  let mut w = ctor(tx).unwrap();

  // While the file is open, windows won't report modified events for it.
  // Flushing doesn't help.  So make sure it is closed before we validate.
  let path = {
      let mut file = NamedTempFile::new().unwrap();
      w.watch(file.path()).unwrap();
      thread::sleep(Duration::from_millis(1000)); // give watcher enough time to spin up

      // make some files that should be exlcuded from watch. this works because tempfile creates
      // them all in the same directory.
      let mut excluded_file = NamedTempFile::new().unwrap();
      let another_excluded_file = NamedTempFile::new().unwrap();
      let _ = another_excluded_file; // eliminate warning
      excluded_file.write_all(b"shouldn't get an event for this").unwrap();

      file.write_all(b"foo").unwrap();
      file.flush().unwrap();
      file.path().to_path_buf()
  };
  let events = validate_recv(rx, vec![(path.as_path(), op::WRITE),
                                      (path.as_path(), op::REMOVE)]);

  // make sure that ONLY the target path is in the list of received events
  for evt in events {
      assert!(evt.path.unwrap() == path.as_path());
  }
}

#[cfg(not(target_os = "windows"))]
fn validate_watch_single_file<F, W>(ctor: F) where
  F: Fn(Sender<Event>) -> Result<W>, W: Watcher {
  let mut file = NamedTempFile::new().unwrap();
  let (tx, rx) = channel();
  let mut w = ctor(tx).unwrap();
  w.watch(file.path()).unwrap();
  thread::sleep(Duration::from_millis(1000));
  file.write_all(b"foo").unwrap();
  file.flush().unwrap();
  validate_recv(rx, vec![(resolve_path(file.path()).as_path(), op::CREATE)]);
}


#[cfg(not(target_os = "linux"))]
fn validate_watch_dir<F, W>(ctor: F) where
  F: Fn(Sender<Event>) -> Result<W>, W: Watcher {
  let dir = TempDir::new("dir").unwrap();
  let dir1 = TempDir::new_in(dir.path(), "dir1").unwrap();
  let dir2 = TempDir::new_in(dir.path(), "dir2").unwrap();
  let dir11 = TempDir::new_in(dir1.path(), "dir11").unwrap();
  let (tx, rx) = channel();
  // OSX FsEvent needs some time to discard old events from its log.
  thread::sleep(Duration::from_millis(12000));
  let mut w = ctor(tx).unwrap();

  w.watch(dir.path()).unwrap();

  let f111 = NamedTempFile::new_in(dir11.path()).unwrap();
  let f111_path = f111.path().to_owned();
  let f111_path = f111_path.as_path();
  let f21 = NamedTempFile::new_in(dir2.path()).unwrap();
  thread::sleep(Duration::from_millis(4000));
  f111.close().unwrap();
  thread::sleep(Duration::from_millis(4000));
  validate_recv(rx, vec![(resolve_path(f111_path).as_path(), op::CREATE),
                         (resolve_path(f21.path()).as_path(), op::CREATE),
                         (resolve_path(f111_path).as_path(), op::REMOVE)]);
}

// Broken on some Linuxen.
#[cfg(not(target_os = "linux"))]
#[test]
fn watch_single_file_recommended() {
  validate_watch_single_file(RecommendedWatcher::new);
}

// Broken on some Linuxen.
#[cfg(not(target_os = "linux"))]
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

#[cfg(target_os = "linux")]
#[test]
fn new_inotify() {
  let (tx, _) = channel();
  let w: Result<INotifyWatcher> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[cfg(target_os = "macos")]
#[test]
fn new_fsevent() {
  let (tx, _) = channel();
  let w: Result<FsEventWatcher> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[test]
fn new_null() {
  let (tx, _) = channel();
  let w: Result<NullWatcher> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[test]
fn new_poll() {
  let (tx, _) = channel();
  let w: Result<PollWatcher> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}

#[test]
fn new_recommended() {
  let (tx, _) = channel();
  let w: Result<RecommendedWatcher> = Watcher::new(tx);
  match w {
    Ok(_) => assert!(true),
    Err(_) => assert!(false)
  }
}
