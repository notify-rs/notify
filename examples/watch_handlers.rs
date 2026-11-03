use notify::{Event, EventHandler, RecursiveMode, WatchPathConfig, Watcher};
use std::{collections::HashMap, path::PathBuf, sync::mpsc};

fn main() -> notify::Result<()> {
    let paths: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("Usage: cargo run -p examples --example watch_handlers -- PATH...");
        return Ok(());
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    let mut handlers: HashMap<u64, Box<dyn EventHandler>> = HashMap::new();
    for (id, path) in paths.into_iter().enumerate() {
        let id = id as u64;
        watcher.watch_with(
            &path,
            WatchPathConfig::new(RecursiveMode::Recursive).with_user_data(id),
        )?;
        handlers.insert(
            id,
            Box::new(move |result: notify::Result<Event>| {
                println!("{}: {result:?}", path.display());
            }),
        );
    }

    for result in rx {
        let user_data = match &result {
            Ok(event) => event.user_data(),
            Err(error) => error.user_data(),
        };
        if let Some(handler) = user_data.and_then(|id| handlers.get_mut(&id)) {
            handler.handle_event(result);
        } else {
            // Unattributed errors and rescan notifications need shared handling.
            eprintln!("Shared notification: {result:?}");
        }
    }
    Ok(())
}
