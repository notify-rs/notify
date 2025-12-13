/// Example demonstrating EventKindMask for filtering filesystem events.
///
/// EventKindMask allows you to configure which types of events you want to receive,
/// reducing noise and improving performance by filtering at the kernel level (on Linux).
///
/// Run with: cargo run --example event_filtering -- <path>
use notify::{Config, EventKindMask, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let path = std::env::args()
        .nth(1)
        .expect("Argument 1 needs to be a path");

    log::info!("Watching {path}");

    // Choose which event filtering mode to demonstrate
    let mode = std::env::args().nth(2).unwrap_or_default();
    let result = match mode.as_str() {
        "core" => {
            log::info!("Mode: CORE (excludes access events like OPEN/CLOSE)");
            watch_core(&path)
        }
        "create-remove" => {
            log::info!("Mode: CREATE | REMOVE only");
            watch_create_remove(&path)
        }
        _ => {
            log::info!("Mode: ALL (default, receives all events)");
            log::info!("  Use 'core' or 'create-remove' as 2nd arg for other modes");
            watch_all(&path)
        }
    };

    if let Err(error) = result {
        log::error!("Error: {error:?}");
    }
}

/// Watch with ALL events (default behavior, backward compatible)
fn watch_all<P: AsRef<Path>>(path: P) -> notify::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();

    // Default config receives all events including access events (OPEN, CLOSE)
    let config = Config::default();
    // Equivalent to: Config::default().with_event_kinds(EventKindMask::ALL)

    let mut watcher = RecommendedWatcher::new(tx, config)?;
    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;

    for res in rx {
        match res {
            Ok(event) => log::info!("Event: {event:?}"),
            Err(error) => log::error!("Error: {error:?}"),
        }
    }

    Ok(())
}

/// Watch with CORE events only (excludes noisy access events)
///
/// CORE includes: CREATE, REMOVE, MODIFY_DATA, MODIFY_META, MODIFY_NAME
/// Excludes: ACCESS_OPEN, ACCESS_CLOSE, ACCESS_CLOSE_NOWRITE
fn watch_core<P: AsRef<Path>>(path: P) -> notify::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();

    // CORE mask excludes access events, reducing noise significantly
    let config = Config::default().with_event_kinds(EventKindMask::CORE);

    let mut watcher = RecommendedWatcher::new(tx, config)?;
    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;

    for res in rx {
        match res {
            Ok(event) => log::info!("Event: {event:?}"),
            Err(error) => log::error!("Error: {error:?}"),
        }
    }

    Ok(())
}

/// Watch only file creation and removal events
fn watch_create_remove<P: AsRef<Path>>(path: P) -> notify::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();

    // Custom mask: only CREATE and REMOVE events
    let config =
        Config::default().with_event_kinds(EventKindMask::CREATE | EventKindMask::REMOVE);

    let mut watcher = RecommendedWatcher::new(tx, config)?;
    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;

    for res in rx {
        match res {
            Ok(event) => log::info!("Event: {event:?}"),
            Err(error) => log::error!("Error: {error:?}"),
        }
    }

    Ok(())
}
