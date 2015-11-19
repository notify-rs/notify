extern crate notify;
extern crate tempdir;
extern crate tempfile;
extern crate time;


use notify::*;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver};
use tempdir::TempDir;
use tempfile::NamedTempFile;

#[cfg(target_os="windows")]
#[test]
fn shutdown() {
    // create a watcher for n directories.  start the watcher, then shut it down.  inspect
    // the watcher to make sure that it received final callbacks for all n watchers.

    let mut dirs:Vec<tempdir::TempDir> = Vec::new();
    let dir_count = 100;

    // to get meta events, we have to pass in the meta channel
    let (meta_tx,meta_rx) = channel();

    {
        let (tx, _) = channel();
        let mut w = ReadDirectoryChangesWatcher::create(tx,meta_tx).unwrap();

        for _ in 0..dir_count {
            let d = TempDir::new("d").unwrap();
            //println!("{:?}", d.path());
            w.watch(d.path()).unwrap();
            dirs.push(d);
        }
    }

    const TIMEOUT_S: f64 = 4.0;
    let deadline = time::precise_time_s() + TIMEOUT_S;
    let mut watchers_shutdown = 0;
    while time::precise_time_s() < deadline {
        if let Ok(actual) = meta_rx.try_recv() {
            match actual {
                WatcherComplete => watchers_shutdown += 1
            }
        }
        thread::sleep_ms(50);
    }

    assert_eq!(watchers_shutdown,dir_count);
}
