use std::{path::Path, time::Duration};

use notify::{RecursiveMode, Watcher};
use notify_debouncer_mini::new_debouncer;

fn main() {
    std::thread::spawn(|| {
        let path = Path::new("test.txt");
        let _ = std::fs::remove_file(&path);
        loop {
            std::fs::write(&path, b"Lorem ipsum").unwrap();
            std::thread::sleep(Duration::from_millis(250));
        }
    });

    let (rx, mut watcher) = new_debouncer(Duration::from_secs(2), None).unwrap();

    watcher
        .watch(Path::new("."), RecursiveMode::Recursive)
        .unwrap();

    for events in rx {
        for e in events {
            println!("{:?}", e);
        }
    }
}
