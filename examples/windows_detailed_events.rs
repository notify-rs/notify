use notify::{Config, Watcher};

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();

    let path = std::env::args()
        .nth(1)
        .expect("Argument 1 needs to be a path");

    log::info!("Watching {path}");

    let (tx, rx) = std::sync::mpsc::channel();

    let config = Config::default().with_windows_detailed_events(true);

    let mut watcher = notify::RecommendedWatcher::new(tx, config).unwrap();
    watcher.watch(path.as_ref(), notify::RecursiveMode::Recursive).unwrap();

    for res in rx {
        match res {
            Ok(event) => log::info!("Event: {event:?}"),
            Err(error) => log::error!("Error: {error:?}"),
        }
    }
}