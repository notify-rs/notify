use std::{path::Path, time::Duration};

use notify::{poll::PollWatcherConfig, *};
fn main() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher: Box<dyn Watcher> = if RecommendedWatcher::kind() == WatcherKind::PollWatcher {
        // custom config for PollWatcher kind
        let config = PollWatcherConfig {
            poll_interval: Duration::from_secs(1),
            ..Default::default()
        };
        Box::new(PollWatcher::with_config(tx, config).unwrap())
    } else {
        // use default config for everything else
        Box::new(RecommendedWatcher::new(tx).unwrap())
    };

    // watch some stuff
    watcher
        .watch(Path::new("."), RecursiveMode::Recursive)
        .unwrap();

    // just print all events, this blocks forever
    for e in rx {
        println!("{:?}", e);
    }
}
