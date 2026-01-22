use std::{path::Path, time::Duration};

use notify::{EventKindMask, RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer_opt, Config};

/// Example for debouncer mini with event filtering.
///
/// This demonstrates using Config::with_notify_config() to pass a custom notify::Config
/// that filters events at the kernel level (on Linux), reducing noise.
fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("debouncer_mini=trace"),
    )
    .init();
    // emit some events by changing a file
    std::thread::spawn(|| {
        let path = Path::new("test.txt");
        let _ = std::fs::remove_file(path);
        // log::info!("running 250ms events");
        for _ in 0..20 {
            log::trace!("writing..");
            std::fs::write(path, b"Lorem ipsum").unwrap();
            std::thread::sleep(Duration::from_millis(250));
        }
        // log::debug!("waiting 20s");
        std::thread::sleep(Duration::from_millis(20000));
        // log::info!("running 3s events");
        for _ in 0..20 {
            // log::debug!("writing..");
            std::fs::write(path, b"Lorem ipsum").unwrap();
            std::thread::sleep(Duration::from_millis(3000));
        }
    });

    // setup debouncer with custom event filtering
    let (tx, rx) = std::sync::mpsc::channel();

    // Configure debouncer with notify config that excludes access events
    // CORE mask: CREATE, REMOVE, MODIFY_DATA, MODIFY_META, MODIFY_NAME
    let config = Config::default()
        .with_timeout(Duration::from_secs(1))
        .with_notify_config(notify::Config::default().with_event_kinds(EventKindMask::CORE));

    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher>(config, tx).unwrap();

    debouncer
        .watcher()
        .watch(Path::new("."), RecursiveMode::Recursive)
        .unwrap();

    // print all events, non returning
    for result in rx {
        match result {
            Ok(events) => events
                .iter()
                .for_each(|event| log::info!("Event {event:?}")),
            Err(error) => log::info!("Error {error:?}"),
        }
    }
}
