#![cfg(all(target_os = "macos", not(feature = "macos_kqueue")))]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use notify::{Config, FsEventWatcher, RecursiveMode, Watcher};

/// Regression test for notify-rs/notify#552.
///
/// The original bug was a non-deterministic segfault on macOS in FSEvents when
/// streams were rapidly opened/closed while a lot of filesystem activity was
/// happening.
#[test]
fn fsevents_rapid_open_close_with_background_events_does_not_segfault() {
    // Keep this reasonably small; the failure mode is a crash, not an assertion.
    const WATCHER_THREADS: usize = 8;
    const DURATION: Duration = Duration::from_secs(2);
    const FILE_SLOTS: usize = 128;

    let tmpdir = tempfile::tempdir().expect("tempdir");
    let root: PathBuf = tmpdir.path().to_path_buf();

    let stop = Arc::new(AtomicBool::new(false));

    // Background filesystem activity.
    let stop_writer = stop.clone();
    let root_writer = root.clone();
    let writer = thread::Builder::new()
        .name("notify-rs test-fsevents-writer".to_string())
        .spawn(move || {
            let mut i = 0usize;
            while !stop_writer.load(Ordering::Relaxed) {
                let slot = i % FILE_SLOTS;
                let path = root_writer.join(format!("slot-{slot}"));

                // Best-effort; if the file disappears due to races, ignore errors.
                let _ = std::fs::write(&path, b"x");
                let _ = std::fs::remove_file(&path);

                i += 1;
                if i % 1024 == 0 {
                    thread::yield_now();
                }
            }
        })
        .expect("spawn writer thread");

    // Rapidly open/close watchers while events are being produced.
    let deadline = Instant::now() + DURATION;
    let mut handles = Vec::with_capacity(WATCHER_THREADS);
    for t in 0..WATCHER_THREADS {
        let root = root.clone();
        handles.push(thread::Builder::new()
            .name(format!("notify-rs test-fsevents-watcher-{t}"))
            .spawn(move || {
                while Instant::now() < deadline {
                    let mut watcher =
                        FsEventWatcher::new(|_| {}, Config::default()).expect("new watcher");
                    watcher
                        .watch(&root, RecursiveMode::Recursive)
                        .expect("watch");

                    // Give the stream a brief moment to start and (potentially) buffer events.
                    thread::sleep(Duration::from_millis(1));
                }
            })
            .expect("spawn watcher thread"));
    }

    for h in handles {
        h.join().expect("watcher thread");
    }

    stop.store(true, Ordering::Relaxed);
    writer.join().expect("writer thread");
}

