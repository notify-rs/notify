use std::{fs, thread, time::Duration};

use notify::{EventKindMask, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{Config, RecommendedCache, new_debouncer_opt, notify};
use tempfile::tempdir;

/// Full debouncer with an explicit backend event mask.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let dir_path = dir.path().to_path_buf();

    // emit some events by changing a file
    thread::spawn(move || {
        let mut n = 1;
        let mut file_path = dir_path.join(format!("file-{n:03}.txt"));
        loop {
            for _ in 0..5 {
                fs::write(&file_path, b"Lorem ipsum").unwrap();
                thread::sleep(Duration::from_millis(500));
            }
            n += 1;
            let target_path = dir_path.join(format!("file-{n:03}.txt"));
            fs::rename(&file_path, &target_path).unwrap();
            file_path = target_path;
        }
    });

    let (tx, rx) = std::sync::mpsc::channel();

    // Preserve the debouncer's CORE mask when replacing its backend config.
    let notify_config = notify::Config::default().with_event_kinds(EventKindMask::CORE);

    // The tick rate is left at its default, 1/4 of the timeout
    let config = Config::default()
        .with_timeout(Duration::from_secs(2))
        .with_notify_config(notify_config);

    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, RecommendedCache>(
        config,
        tx,
        RecommendedCache::new(),
    )?;

    debouncer.watch(dir.path(), RecursiveMode::Recursive)?;

    for result in rx {
        match result {
            Ok(events) => events.iter().for_each(|event| println!("{event:?}")),
            Err(errors) => errors.iter().for_each(|error| println!("{error:?}")),
        }
        println!();
    }

    Ok(())
}
