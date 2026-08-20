use notify::{Config, PollWatcher, RecursiveMode, Watcher, poll::ScanEvent};
use std::path::Path;

// Example for the pollwatcher scan callback feature.
// Returns the scanned paths
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let path = std::env::args()
        .nth(1)
        .expect("Argument 1 needs to be a path");

    log::info!("Watching {path}");

    if let Err(error) = watch(path) {
        log::error!("Error: {error:?}");
    }
}

fn watch<P: AsRef<Path>>(path: P) -> notify::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();

    // if you want to use the same channel for both events
    // and you need to differentiate between scan and file change events,
    // then you will have to use something like this
    enum Message {
        Event(notify::Result<notify::Event>),
        Scan(ScanEvent),
    }

    let tx_c = tx.clone();
    // use the pollwatcher and set a callback for the scanning events
    let mut watcher = PollWatcher::with_initial_scan(
        move |watch_event| {
            tx_c.send(Message::Event(watch_event)).unwrap();
        },
        Config::default(),
        move |scan_event| {
            tx.send(Message::Scan(scan_event)).unwrap();
        },
    )?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;

    for res in rx {
        match res {
            Message::Event(e) => println!("Watch event {e:?}"),
            Message::Scan(e) => println!("Scan event {e:?}"),
        }
    }

    Ok(())
}
