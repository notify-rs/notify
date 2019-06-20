#![allow(unused_macros)]

use crossbeam_channel::{Receiver, TryRecvError};
use notify::*;
use tempdir::TempDir;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::PermissionsExt;

#[cfg(not(target_os = "windows"))]
const TIMEOUT_MS: u64 = 100;
#[cfg(target_os = "windows")]
const TIMEOUT_MS: u64 = 3000; // windows can take a while

pub fn recv_events_with_timeout(
    rx: &Receiver<Result<Event>>,
    timeout: Duration,
) -> Vec<Event> {
    let start = Instant::now();
    let mut evs = Vec::new();

    while start.elapsed() < timeout {
        match rx.try_recv() {
            Ok(Ok(event)) => { evs.push(event); },
            Ok(Err(err)) => panic!("unexpected event err: {:?}", err),
            Err(TryRecvError::Empty) => (),
            Err(e) => panic!("unexpected channel err: {:?}", e),
        }

        sleep(1);
    }

    evs
}

pub fn recv_events(rx: &Receiver<Result<Event>>) -> Vec<Event> {
    recv_events_with_timeout(rx, Duration::from_millis(TIMEOUT_MS))
}

/// Simple kind with only the top level of the event classification
#[derive(Debug, PartialEq, PartialOrd, Eq, Hash, Ord, Clone, Copy)]
pub enum Kind {
    Any,
    Access,
    Create,
    Rename,
    Metadata,
    Modify,
    Remove,
    Other,
}

impl From<EventKind> for Kind {
    fn from(k: EventKind) -> Self {
        match k {
            EventKind::Any => Kind::Any,
            EventKind::Access(_) => Kind::Access,
            EventKind::Create(_) => Kind::Create,
            EventKind::Modify(event::ModifyKind::Name(_)) => Kind::Rename,
            EventKind::Modify(event::ModifyKind::Metadata(_)) => Kind::Metadata,
            EventKind::Modify(_) => Kind::Modify,
            EventKind::Remove(_) => Kind::Remove,
            EventKind::Other => Kind::Other,
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Hash, Ord, Clone)]
pub struct SlimEvent {
    pub paths: Vec<PathBuf>,
    pub kind: Kind,
    pub tracker: Option<usize>,
}

impl SlimEvent {
    pub fn new<P>(paths: &[P], kind: Kind, tracker: Option<usize>) -> Self where P: AsRef<Path> {
        Self { paths: paths.into_iter().map(|p| p.as_ref().into()).collect(), kind, tracker }
    }

    pub fn one<P>(paths: &[P], kind: Kind, tracker: Option<usize>) -> Vec<Self> where P: AsRef<Path> {
        vec![Self::new(paths, kind, tracker)]
    }

    pub fn seq<P>(evs: &[(&[P], Kind, Option<usize>)]) -> Vec<Self> where P: AsRef<Path> {
        evs.into_iter().map(|(paths, kind, tracker)| Self::new(paths, *kind, *tracker)).collect()
    }
}

/// Converts full-descriptor events into a simplified representation
pub fn slim(input: Vec<Event>) -> Vec<SlimEvent> {
    input.into_iter().map(|e| SlimEvent {
        tracker: e.tracker(),
        kind: Kind::from(e.kind),
        paths: e.paths,
    }).collect()
}

// FSEvents tends to emit events multiple times and aggregate events,
// so just check that all expected events arrive for each path,
// and make sure the paths are in the correct order
// TODO: remove
pub fn inflate_events(input: Vec<(PathBuf, Op, Option<u32>)>) -> Vec<(PathBuf, Op, Option<u32>)> {
    let mut output = Vec::new();
    let mut path = None;
    let mut ops = Op::empty();
    let mut cookie = None;
    for (e_p, e_o, e_c) in input {
        let p = match path {
            Some(p) => p,
            None => e_p.clone(),
        };
        let c = match cookie {
            Some(c) => Some(c),
            None => e_c,
        };
        if p == e_p && c == e_c {
            ops |= e_o;
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

// TODO: remove
pub fn extract_cookies(events: &[(PathBuf, Op, Option<u32>)]) -> Vec<u32> {
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

// Sleep for `duration` in milliseconds if running on macOS
pub fn sleep_macos(duration: u64) {
    if cfg!(target_os = "macos") {
        sleep(duration)
    }
}

// Sleep for `duration` in milliseconds if running on Windows
pub fn sleep_windows(duration: u64) {
    if cfg!(target_os = "windows") {
        sleep(duration)
    }
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
    /// Toggle "other" rights on linux and macOS and "readonly" on windows
    fn modify_metadata(&self, p: &str);
    /// Write some data to a file
    fn modify_data(&self, p: &str);
    /// Remove file or directory
    fn remove(&self, p: &str);
}

impl TestHelpers for TempDir {
    fn mkpath(&self, p: &str) -> PathBuf {
        let mut path = self
            .path()
            .canonicalize()
            .expect("failed to canonicalize path")
            .to_owned();
        for part in p.split('/').collect::<Vec<_>>() {
            if part != "." {
                path.push(part);
            }
        }
        path
    }

    fn create(&self, p: &str) {
        let path = self.mkpath(p);
        if path
            .components()
            .last()
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap()
            .contains("dir")
        {
            fs::create_dir_all(path).expect("failed to create directory");
        } else {
            let parent = path
                .parent()
                .expect("failed to get parent directory")
                .to_owned();
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

    #[cfg(not(target_os = "windows"))]
    fn modify_metadata(&self, p: &str) {
        let path = self.mkpath(p);
        let mut permissions = fs::metadata(&path)
            .expect("failed to get metadata")
            .permissions();
        let u = (permissions.mode() / 100) % 10;
        let g = (permissions.mode() / 10) % 10;
        let o = if permissions.mode() % 10 == 0 { g } else { 0 };
        permissions.set_mode(u * 100 + g * 10 + o);
        fs::set_permissions(path, permissions).expect("failed to chmod file or directory");
    }

    #[cfg(target_os = "windows")]
    fn modify_metadata(&self, p: &str) {
        let path = self.mkpath(p);
        let mut permissions = fs::metadata(&path)
            .expect("failed to get metadata")
            .permissions();
        let r = permissions.readonly();
        permissions.set_readonly(!r);
        fs::set_permissions(path, permissions).expect("failed to chmod file or directory");
    }

    fn modify_data(&self, p: &str) {
        let path = self.mkpath(p);

        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("failed to open file");

        file.write(b"some data").expect("failed to write to file");
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

macro_rules! assert_eq_any {
    ($left:expr, $right1:expr, $right2:expr) => ({
        match (&($left), &($right1), &($right2)) {
            (left_val, right1_val, right2_val) => {
                if *left_val != *right1_val && *left_val != *right2_val {
                    panic!("assertion failed: `(left != right1 or right2)` (left: `{:?}`, right1: `{:?}`, right2: `{:?}`)", left_val, right1_val, right2_val)
                }
            }
        }
    })
}
