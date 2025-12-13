use std::{fs, thread, time::Duration};

use notify::{EventKindMask, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer_opt, notify, RecommendedCache};
use tempfile::tempdir;

/// Advanced example of the notify-debouncer-full with event filtering.
///
/// This demonstrates using new_debouncer_opt() to pass a custom notify::Config
/// that filters events at the kernel level (on Linux), reducing noise and
/// improving performance.
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

    // setup debouncer with custom event filtering
    let (tx, rx) = std::sync::mpsc::channel();

    // Configure notify to exclude noisy access events (OPEN/CLOSE)
    // Use CORE mask: CREATE, REMOVE, MODIFY_DATA, MODIFY_META, MODIFY_NAME
    let notify_config = notify::Config::default().with_event_kinds(EventKindMask::CORE);

    // Use new_debouncer_opt for full control over the watcher configuration
    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, RecommendedCache>(
        Duration::from_secs(2), // debounce timeout
        None,                   // tick rate (None = auto)
        tx,
        RecommendedCache::new(),
        notify_config,
    )?;

    debouncer.watch(dir.path(), RecursiveMode::Recursive)?;

    // print all events and errors
    for result in rx {
        match result {
            Ok(events) => events.iter().for_each(|event| println!("{event:?}")),
            Err(errors) => errors.iter().for_each(|error| println!("{error:?}")),
        }
        println!();
    }

    Ok(())
}
