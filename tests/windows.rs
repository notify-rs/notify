extern crate notify;
extern crate tempdir;
extern crate tempfile;
extern crate time;

use notify::*;
use std::thread;
use std::sync::mpsc::{channel, Receiver};
use tempdir::TempDir;

fn check_for_error(rx:&Receiver<notify::Event>) {
    while let Ok(res) = rx.try_recv() {
        match res.op {
            Err(e) => panic!("unexpected err: {:?}: {:?}", e, res.path),
            _ => ()
        }
    };
}
#[cfg(target_os="windows")]
#[test]
fn shutdown() {
    // create a watcher for n directories.  start the watcher, then shut it down.  inspect
    // the watcher to make sure that it received final callbacks for all n watchers.
    let dir_count = 100;

    // to get meta events, we have to pass in the meta channel
    let (meta_tx,meta_rx) = channel();
    let (tx, rx) = channel();
    {
        let mut dirs:Vec<tempdir::TempDir> = Vec::new();
        let mut w = ReadDirectoryChangesWatcher::create(tx,meta_tx).unwrap();

        for _ in 0..dir_count {
            let d = TempDir::new("rsnotifytest").unwrap();
            dirs.push(d);
        }

        for d in &dirs { // need the ref, otherwise its a move and the dir will be dropped!
            //println!("{:?}", d.path());
            w.watch(d.path(), RecursiveMode::Recursive).unwrap();
        }

        // unwatch half of the directories, let the others get stopped when we go out of scope
        for d in &dirs[0..dir_count/2] {
            w.unwatch(d.path()).unwrap();
        }

        thread::sleep_ms(2000); // sleep to unhook the watches
    }

    check_for_error(&rx);

    const TIMEOUT_S: f64 = 60.0;  // give it PLENTY of time before we declare failure
    let deadline = time::precise_time_s() + TIMEOUT_S;
    let mut watchers_shutdown = 0;
    while watchers_shutdown != dir_count && time::precise_time_s() < deadline {
        if let Ok(actual) = meta_rx.try_recv() {
            match actual {
                notify::windows::MetaEvent::SingleWatchComplete => watchers_shutdown += 1,
                _ => ()
            }
        }
        thread::sleep_ms(50); // don't burn cpu, can take some time for completion events to fire
    }

    assert_eq!(watchers_shutdown,dir_count);
}

#[cfg(target_os="windows")]
#[test]
fn watch_deleted_fails() {
    let pb = {
        let d = TempDir::new("rsnotifytest").unwrap();
        d.path().to_path_buf()
    };

    let (tx, _) = channel();
    let mut w = ReadDirectoryChangesWatcher::new(tx).unwrap();
    match w.watch(pb.as_path(), RecursiveMode::Recursive) {
        Ok(x) => panic!("Should have failed, but got: {:?}", x),
        Err(_) => ()
    }
}

#[cfg(target_os="windows")]
#[test]
fn watch_server_can_be_awakened() {
    let (tx, _) = channel();
    let (meta_tx,meta_rx) = channel();
    let mut w = ReadDirectoryChangesWatcher::create(tx,meta_tx).unwrap();
    let d = TempDir::new("rsnotifytest").unwrap();
    let d2 = TempDir::new("rsnotifytest").unwrap();

    match w.watch(d.path(), RecursiveMode::Recursive) {
        Ok(_) => (),
        Err(e) => panic!("Oops: {:?}", e)
    }
    match w.watch(d2.path(), RecursiveMode::Recursive) {
        Ok(_) => (),
        Err(e) => panic!("Oops: {:?}", e)
    }
    // should be at least one awaken in there
    const TIMEOUT_S: f64 = 5.0;
    let deadline = time::precise_time_s() + TIMEOUT_S;
    let mut awakened = false;
    while time::precise_time_s() < deadline {
        if let Ok(actual) = meta_rx.try_recv() {
            match actual {
                notify::windows::MetaEvent::WatcherAwakened => awakened = true,
                _ => ()
            }
        }
        thread::sleep_ms(50);
    }

    if !awakened {
        panic!("Failed to awaken");
    }
}

#[cfg(target_os="windows")]
#[test]
#[ignore]
// repeatedly watch and unwatch a directory; make sure process memory does not increase.
// you use task manager to watch the memory; it will fluctuate a bit, but should not leak overall
fn memtest_manual() {
    loop {
        let (tx, rx) = channel();
        let d = TempDir::new("rsnotifytest").unwrap();
        {
            let (meta_tx,_) = channel();
            let mut w = ReadDirectoryChangesWatcher::create(tx,meta_tx).unwrap();
            w.watch(d.path(), RecursiveMode::Recursive).unwrap();
            thread::sleep_ms(1); // this should make us run pretty hot but not insane
        }
        check_for_error(&rx);
    }
}
