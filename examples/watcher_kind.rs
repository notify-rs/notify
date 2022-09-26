use std::{path::Path, time::Duration};
use notify::*;

// example of detecting the recommended watcher kind
fn main() {
    let (tx, rx) = std::sync::mpsc::channel();
    // This example is a little bit misleading as you can just create one Config and use it for all watchers.
    // That way the pollwatcher specific stuff is still configured, if it should be used.
    let mut watcher: Box<dyn Watcher> = if RecommendedWatcher::kind() == WatcherKind::PollWatcher {
        // custom config for PollWatcher kind
        // you 
        let config = Config::default()
            .with_poll_interval(Duration::from_secs(1));
        Box::new(PollWatcher::new(tx, config).unwrap())
    } else {
        // use default config for everything else
        Box::new(RecommendedWatcher::new(tx, Config::default()).unwrap())
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
