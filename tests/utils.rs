extern crate notify;
extern crate time;
extern crate tempdir;

use tempdir::TempDir;

use notify::*;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

#[cfg(not(target_os="windows"))]
use std::os::unix::fs::PermissionsExt;

#[cfg(not(target_os="windows"))]
const TIMEOUT_S: f64 = 0.1;
#[cfg(target_os="windows")]
const TIMEOUT_S: f64 = 3.0; // windows can take a while

pub fn recv_events(rx: &Receiver<Event>) ->  Vec<(PathBuf, Op, Option<u32>)> {
    let deadline = time::precise_time_s() + TIMEOUT_S;

    let mut evs = Vec::new();

    while time::precise_time_s() < deadline {
        match rx.try_recv() {
            Ok(Event{path: Some(path), op: Ok(op), cookie}) => {
                evs.push((path, op, cookie));
            },
            Ok(Event{path: None, ..})  => (),
            Ok(Event{op: Err(e), ..}) => panic!("unexpected event err: {:?}", e),
            Err(TryRecvError::Empty) => (),
            Err(e) => panic!("unexpected channel err: {:?}", e)
        }
    }
    evs
}

// FSEvent tends to emit events multiple times and aggregate events,
// so just check that all expected events arrive for each path,
// and make sure the paths are in the correct order
pub fn inflate_events(input: Vec<(PathBuf, Op, Option<u32>)>) -> Vec<(PathBuf, Op, Option<u32>)> {
    let mut output = Vec::new();
    let mut path = None;
    let mut ops = Op::empty();
    let mut cookie = None;
    for (e_p, e_o, e_c) in input {
        let p = match path {
            Some(p) => p,
            None => e_p.clone()
        };
        let c = match cookie {
            Some(c) => Some(c),
            None => e_c
        };
        if p == e_p && c == e_c {
            // ops |= e_o;
            ops = Op::from_bits_truncate(ops.bits() | e_o.bits());
        } else {
            output.push((p, ops, cookie));
            ops = e_o;
        }
        path = Some(e_p);
        cookie = e_c;
    }
    if let Some(p) = path {
        output.push((p, ops, cookie));
    }
    output
}

pub fn extract_cookies(events: &Vec<(PathBuf, Op, Option<u32>)>) -> Vec<u32> {
    let mut cookies = Vec::new();
    for &(_, _, e_c) in events {
        if let Some(cookie) = e_c {
            if !cookies.contains(&cookie) {
                cookies.push(cookie);
            }
        }
    }
    cookies
}

// Sleep for `duration` in milliseconds
pub fn sleep(duration: u64) {
    thread::sleep(Duration::from_millis(duration));
}

pub trait TestHelpers {
    /// Return path relative to the TempDir. Directory separator must be a forward slash, and will be converted to the platform's native separator.
    fn mkpath(&self, p: &str) -> PathBuf;
    /// Create file or directory. Directories must contain the phrase "dir" otherwise they will be interpreted as files.
    fn create(&self, p: &str);
    /// Create all files and directories in the `paths` list. Directories must contain the phrase "dir" otherwise they will be interpreted as files.
    fn create_all(&self, paths: Vec<&str>);
    /// Rename file or directory.
    fn rename(&self, a: &str, b: &str);
    fn chmod(&self, p: &str);
    fn write(&self, p: &str);
    fn remove(&self, p: &str);
}

impl TestHelpers for TempDir {
    fn mkpath(&self, p: &str) -> PathBuf {
        let mut path = self.path().canonicalize().expect("failed to canonalize path").to_owned();
        for part in p.split('/').collect::<Vec<_>>() {
            if part != "." {
                path.push(part);
            }
        }
        path
    }

    fn create(&self, p: &str) {
        let path = self.mkpath(p);
        if path.components().last().unwrap().as_os_str().to_str().unwrap().contains("dir") {
            fs::create_dir_all(path).expect("failed to create directory");
        } else {
            let parent = path.parent().expect("failed to get parent directory").to_owned();
            if !parent.exists() {
                fs::create_dir_all(parent).expect("failed to create parent directory");
            }
            fs::File::create(path).expect("failed to create file");
        }
    }

    fn create_all(&self, paths: Vec<&str>) {
        for p in paths {
            self.create(p);
        }
    }

    fn rename(&self, a: &str, b: &str) {
        let path_a = self.mkpath(a);
        let path_b = self.mkpath(b);
        fs::rename(&path_a, &path_b).expect("failed to rename file or directory");
    }

    #[cfg(not(target_os="windows"))]
    fn chmod(&self, p: &str) {
        let path = self.mkpath(p);
        fs::set_permissions(path, fs::Permissions::from_mode(777)).expect("failed to chmod file or directory");
    }

    #[cfg(target_os="windows")]
    fn chmod(&self, p: &str) {
        let path = self.mkpath(p);
        let mut permissions = fs::metadata(&path).expect("failed to get metadata").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).expect("failed to chmod file or directory");
    }

    fn write(&self, p: &str) {
        let path = self.mkpath(p);

        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("failed to open file");

        file.write("some data".as_bytes()).expect("failed to write to file");
        file.sync_all().expect("failed to sync file");
    }

    fn remove(&self, p: &str) {
        let path = self.mkpath(p);
        if path.is_dir() {
            fs::remove_dir(path).expect("failed to remove directory");
        } else {
            fs::remove_file(path).expect("failed to remove file");
        }
    }
}
