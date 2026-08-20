use std::{path::Path, time::Duration};

use notify::{self, RecursiveMode};
use notify_debouncer_mini::{Config, new_debouncer_opt};

/// Debouncer with custom backend and waiting for exit
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

    // setup debouncer
    let (tx, rx) = std::sync::mpsc::channel();
    // notify backend configuration
    let backend_config = notify::Config::default().with_poll_interval(Duration::from_secs(1));
    // debouncer configuration
    let debouncer_config = Config::default()
        .with_timeout(Duration::from_millis(1000))
        .with_notify_config(backend_config);
    // select backend via fish operator, here PollWatcher backend
    let mut debouncer = new_debouncer_opt::<_, notify::PollWatcher>(debouncer_config, tx).unwrap();

    debouncer
        .watcher()
        .watch(Path::new("."), RecursiveMode::Recursive)
        .unwrap();
    // print all events, non returning
    for result in rx {
        match result {
            Ok(event) => println!("Event {event:?}"),
            Err(error) => println!("Error {error:?}"),
        }
    }
}
