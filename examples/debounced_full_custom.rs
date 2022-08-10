use std::{path::Path, time::Duration};

use notify::{RecursiveMode, Config};
use notify_debouncer_mini::new_debouncer_opt;

/// Debouncer with custom backend and waiting for exit
fn main() {
    // emit some events by changing a file
    std::thread::spawn(|| {
        let path = Path::new("test.txt");
        let _ = std::fs::remove_file(&path);
        loop {
            std::fs::write(&path, b"Lorem ipsum").unwrap();
            std::thread::sleep(Duration::from_millis(250));
        }
    });

    // setup debouncer
    let (tx, rx) = std::sync::mpsc::channel();
    // select backend via fish operator, here PollWatcher backend
    let mut debouncer = new_debouncer_opt::<_,notify::PollWatcher>(Duration::from_secs(2), None, tx, Config::default()).unwrap();

    debouncer
        .watcher()
        .watch(Path::new("."), RecursiveMode::Recursive)
        .unwrap();
    // print all events, non returning
    for events in rx {
        for e in events {
            println!("{:?}", e);
        }
    }
}
