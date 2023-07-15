use std::{fs, thread, time::Duration};

use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::new_debouncer;
use tempfile::tempdir;

/// Advanced example of the notify-debouncer-full, accessing the internal file ID cache
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

    // setup debouncer
    let (tx, rx) = std::sync::mpsc::channel();

    // no specific tickrate, max debounce time 2 seconds
    let mut debouncer = new_debouncer(Duration::from_secs(2), None, tx)?;

    debouncer
        .watcher()
        .watch(dir.path(), RecursiveMode::Recursive)?;

    debouncer
        .cache()
        .add_root(dir.path(), RecursiveMode::Recursive);

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
