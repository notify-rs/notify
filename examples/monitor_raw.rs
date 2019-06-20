extern crate crossbeam_channel;
extern crate notify;

use crossbeam_channel::unbounded;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;

fn watch<P: AsRef<Path>>(path: P) -> notify::Result<()> {
    // Create a channel to receive the events.
    let (tx, rx) = unbounded();

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let mut watcher: RecommendedWatcher = Watcher::new_immediate(tx)?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(path, RecursiveMode::Recursive)?;

    // This is a simple loop, but you may want to use more complex logic here,
    // for example to handle I/O.
    loop {
        match rx.recv() {
            Ok(notify::RawEvent {
                path: Some(path),
                op: Ok(op),
                cookie,
            }) => println!("{:?} {:?} ({:?})", op, path, cookie),
            Ok(event) => println!("broken event: {:?}", event),
            Err(e) => println!("watch error: {:?}", e),
        }
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("Argument 1 needs to be a path");
    println!("watching {}", path);
    if let Err(e) = watch(path) {
        println!("error: {:?}", e)
    }
}
