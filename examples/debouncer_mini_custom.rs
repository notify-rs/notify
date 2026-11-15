use std::{path::Path, time::Duration};

use notify::{self, RecursiveMode};
use notify_debouncer_mini::{Config, DebouncedEventBatch, new_debouncer_opt};

/// Mini debouncer using PollWatcher.
fn main() {
    // emit some events by changing a file
    std::thread::spawn(|| {
        let path = Path::new("test.txt");
        let _ = std::fs::remove_file(path);
        loop {
            std::fs::write(path, b"Lorem ipsum").unwrap();
            std::thread::sleep(Duration::from_millis(250));
        }
    });

    let (tx, rx) = std::sync::mpsc::channel();
    let backend_config = notify::Config::default().with_poll_interval(Duration::from_secs(1));
    let debouncer_config = Config::default()
        .with_timeout(Duration::from_millis(1000))
        .with_notify_config(backend_config);
    let mut debouncer = new_debouncer_opt::<_, notify::PollWatcher>(debouncer_config, tx).unwrap();

    debouncer
        .watcher()
        .watch(Path::new("."), RecursiveMode::Recursive)
        .unwrap();
    for result in rx {
        match result {
            Ok(DebouncedEventBatch::Events(events)) => {
                for event in events {
                    println!("Event {event:?}");
                }
            }
            Ok(DebouncedEventBatch::Rescan) => {
                // Refresh any cached state for all watched paths here.
                println!("Events were missed; rescan all watched paths");
            }
            Ok(_) => {}
            Err(error) => println!("Error {error:?}"),
        }
    }
}
