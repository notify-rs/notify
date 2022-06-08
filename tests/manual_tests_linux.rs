#![cfg(target_os = "linux")]
#![cfg(feature = "manual_tests")]

use std::{
    fs::File,
    io::prelude::*,
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{unbounded, TryRecvError};
use notify::*;

use utils::*;
mod utils;

const TEMP_DIR: &str = "temp_dir";

#[test]
// Test preparation:
// 1. Run `sudo echo 10 > /proc/sys/fs/inotify/max_queued_events`
// 2. Uncomment the lines near "test inotify_queue_overflow" in inotify watcher
fn inotify_queue_overflow() {
    let mut max_queued_events = String::new();
    let mut f = File::open("/proc/sys/fs/inotify/max_queued_events")
        .expect("failed to open max_queued_events");
    f.read_to_string(&mut max_queued_events)
        .expect("failed to read max_queued_events");
    assert_eq!(max_queued_events.trim(), "10");

    let tdir = tempfile::Builder::new()
        .prefix(TEMP_DIR)
        .tempdir()
        .expect("failed to create temporary directory");

    let (tx, rx) = unbounded();
    let mut watcher: RecommendedWatcher =
        Watcher::new(tx).expect("failed to create recommended watcher");
    watcher
        .watch(&tdir.mkpath("."), RecursiveMode::Recursive)
        .expect("failed to watch directory");

    for i in 0..20 {
        let filename = format!("file{}", i);
        tdir.create(&filename);
        tdir.remove(&filename);
    }

    sleep(100);

    let start = Instant::now();

    let mut rescan_found = false;
    while !rescan_found && start.elapsed().as_secs() < 5 {
        match rx.try_recv() {
            Ok(Err(Error {
                // TRANSLATION: this may not be correct
                kind: ErrorKind::MaxFilesWatch,
                ..
            })) => rescan_found = true,
            Ok(Err(e)) => panic!("unexpected event err: {:?}", e),
            Ok(Ok(_)) => (),
            Err(TryRecvError::Empty) => (),
            Err(e) => panic!("unexpected channel err: {:?}", e),
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert!(rescan_found);
}
