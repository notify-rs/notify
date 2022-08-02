use std::time::Duration;

use notify::{poll::PollWatcherConfig, *};
fn main() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let _watcher: Box<dyn Watcher> = if RecommendedWatcher::kind() == WatcherKind::PollWatcher {
        let config = PollWatcherConfig {
            poll_interval: Duration::from_secs(1),
            ..Default::default()
        };
        Box::new(PollWatcher::with_config(tx, config).unwrap())
    } else {
        Box::new(RecommendedWatcher::new(tx).unwrap())
    };
    // use _watcher here
}
