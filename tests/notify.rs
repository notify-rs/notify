extern crate notify;
extern crate tempdir;
extern crate tempfile;

use notify::*;
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use tempdir::TempDir;
use tempfile::NamedTempFile;

fn validate_recv(rx: Receiver<Event>, evs: Vec<(&Path, Op)>) {
  for expected in evs {
    let actual = rx.recv().unwrap();
    assert_eq!(actual.path.unwrap().as_path(), expected.0);
    assert_eq!(actual.op.unwrap(), expected.1);
  }
  assert!(rx.try_recv().is_err());
}

#[test]
fn watch_single_file() {
  let mut file = NamedTempFile::new().unwrap();
  let (tx, rx) = channel();
  let mut w: RecommendedWatcher = Watcher::new(tx).unwrap();
  w.watch(file.path()).unwrap();
  file.write_all(b"foo").unwrap();
  file.flush().unwrap();
  validate_recv(rx, vec![(file.path(), op::WRITE)]);
}

#[test]
fn watch_dir() {
  let dir = TempDir::new("dir").unwrap();
  let dir1 = TempDir::new_in(dir.path(), "dir1").unwrap();
  let dir2 = TempDir::new_in(dir.path(), "dir2").unwrap();
  let dir11 = TempDir::new_in(dir1.path(), "dir11").unwrap();
  let (tx, rx) = channel();
  let mut w: RecommendedWatcher = Watcher::new(tx).unwrap();
  w.watch(dir.path()).unwrap();
  let f111 = NamedTempFile::new_in(dir11.path()).unwrap();
  let f111_path = f111.path().to_owned();
  let f111_path = f111_path.as_path();
  let f21 = NamedTempFile::new_in(dir2.path()).unwrap();
  f111.close().unwrap();
  validate_recv(rx, vec![(f111_path, op::CREATE),
                         (f21.path(), op::CREATE),
                         (f111_path, op::REMOVE)]);
}
