use std::{path::Path, time::Duration};

use notify::{EventKindMask, RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{Config, DebouncedEventBatch, new_debouncer_opt};

/// Mini debouncer with an explicit backend event mask.
fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("debouncer_mini=trace"),
    )
    .init();
    // emit some events by changing a file
    std::thread::spawn(|| {
        let path = Path::new("test.txt");
        let _ = std::fs::remove_file(path);
        for _ in 0..20 {
            log::trace!("writing..");
            std::fs::write(path, b"Lorem ipsum").unwrap();
            std::thread::sleep(Duration::from_millis(250));
        }
        std::thread::sleep(Duration::from_millis(20000));
        for _ in 0..20 {
            std::fs::write(path, b"Lorem ipsum").unwrap();
            std::thread::sleep(Duration::from_millis(3000));
        }
    });

    let (tx, rx) = std::sync::mpsc::channel();

    // Preserve the debouncer's CORE mask when replacing its backend config.
    let config = Config::default()
        .with_timeout(Duration::from_secs(1))
        .with_notify_config(notify::Config::default().with_event_kinds(EventKindMask::CORE));

    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher>(config, tx).unwrap();

    debouncer
        .watcher()
        .watch(Path::new("."), RecursiveMode::Recursive)
        .unwrap();

    for result in rx {
        match result {
            Ok(DebouncedEventBatch::Events(events)) => events
                .iter()
                .for_each(|event| log::info!("Event {event:?}")),
            Ok(DebouncedEventBatch::Rescan) => {
                // Refresh any cached state for all watched paths here.
                log::warn!("Events were missed; rescan all watched paths");
            }
            Ok(_) => {}
            Err(error) => log::info!("Error {error:?}"),
        }
    }
}
