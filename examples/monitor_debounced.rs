use notify::{Watcher, RecursiveMode};
use notify_debouncer_full::new_debouncer;
use std::{path::Path, time::Duration};

/// Example for notify-debouncer-full
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

    // Create a new debounced file watcher with a timeout of 2 seconds.
    // The tickrate will be selected automatically, as well as the underlying watch implementation.
    let mut debouncer = new_debouncer(Duration::from_secs(2), None, tx)?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    debouncer.watcher().watch(path.as_ref(), RecursiveMode::Recursive)?;

    // print all events and errors
    for result in rx {
        match result {
            Ok(events) => events.iter().for_each(|event| log::info!("{event:?}")),
            Err(errors) => errors.iter().for_each(|error| log::error!("{error:?}")),
        }
    }

    Ok(())
}
