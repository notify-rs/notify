#![allow(dead_code)]

extern crate crossbeam_channel;
extern crate notify;
extern crate tempdir;

#[cfg(target_os = "windows")]
mod windows_tests {
    use super::notify::*;
    use super::tempdir::TempDir;
    use crossbeam_channel::{unbounded, Receiver, TryRecvError};
    use std::thread;
    use std::time::{Duration, Instant};

    fn wait_for_disconnect(rx: &Receiver<RawEvent>) {
        loop {
            match rx.try_recv() {
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => (),
                Ok(res) => match res.op {
                    Err(e) => panic!("unexpected err: {:?}: {:?}", e, res.path),
                    _ => (),
                },
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn shutdown() {
        // create a watcher for n directories.  start the watcher, then shut it down.  inspect
        // the watcher to make sure that it received final callbacks for all n watchers.
        let dir_count = 100;

        // to get meta events, we have to pass in the meta channel
        let (meta_tx, meta_rx) = unbounded();
        let (tx, rx) = unbounded();
        {
            let mut dirs: Vec<TempDir> = Vec::new();
            let mut w = ReadDirectoryChangesWatcher::create(tx, meta_tx).unwrap();

            for _ in 0..dir_count {
                let d = TempDir::new("rsnotifytest").unwrap();
                dirs.push(d);
            }

            // need the ref, otherwise it's a move and the dir will be dropped!
            for d in &dirs {
                w.watch(d.path(), RecursiveMode::Recursive).unwrap();
            }

            // unwatch half of the directories, let the others get stopped when we go out of scope
            for d in &dirs[0..dir_count / 2] {
                w.unwatch(d.path()).unwrap();
            }

            thread::sleep(Duration::from_millis(2000)); // sleep to unhook the watches
        }

        wait_for_disconnect(&rx);

        const TIMEOUT_MS: u64 = 60000; // give it PLENTY of time before we declare failure
        let start = Instant::now();

        let mut watchers_shutdown = 0;
        while watchers_shutdown != dir_count && start.elapsed() < Duration::from_millis(TIMEOUT_MS)
        {
            if let Ok(actual) = meta_rx.try_recv() {
                match actual {
                    windows::MetaEvent::SingleWatchComplete => watchers_shutdown += 1,
                    _ => (),
                }
            }
            thread::sleep(Duration::from_millis(1)); // don't burn cpu, can take some time for completion events to fire
        }

        assert_eq!(dir_count, watchers_shutdown);
    }

    #[test]
    fn watch_server_can_be_awakened() {
        let (tx, _) = unbounded();
        let (meta_tx, meta_rx) = unbounded();
        let mut w = ReadDirectoryChangesWatcher::create(tx, meta_tx).unwrap();

        let d = TempDir::new("rsnotifytest").unwrap();
        w.watch(d.path(), RecursiveMode::Recursive).unwrap();

        // should be at least one awaken in there
        const TIMEOUT_MS: u64 = 5000;
        let start = Instant::now();

        let mut awakened = false;
        while !awakened && start.elapsed() < Duration::from_millis(TIMEOUT_MS) {
            if let Ok(actual) = meta_rx.try_recv() {
                match actual {
                    windows::MetaEvent::WatcherAwakened => awakened = true,
                    _ => (),
                }
            }
            thread::sleep(Duration::from_millis(50));
        }

        assert!(awakened);
    }

    #[test]
    #[ignore]
    #[cfg(feature = "manual_tests")]
    // repeatedly watch and unwatch a directory; make sure process memory does not increase.
    // you use task manager to watch the memory; it will fluctuate a bit, but should not leak overall
    fn memtest_manual() {
        let mut i = 0;
        loop {
            let (tx, rx) = unbounded();
            let d = TempDir::new("rsnotifytest").unwrap();
            {
                let (meta_tx, _) = unbounded();
                let mut w = ReadDirectoryChangesWatcher::create(tx, meta_tx).unwrap();
                w.watch(d.path(), RecursiveMode::Recursive).unwrap();
                thread::sleep(Duration::from_millis(1)); // this should make us run pretty hot but not insane
            }
            wait_for_disconnect(&rx);
            i += 1;
            println!("memtest {}", i);
        }
    }
}
