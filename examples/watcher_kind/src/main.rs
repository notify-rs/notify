use std::time::Duration;

use notify::*;
fn main() {
    let (tx, rx) = std::sync::mpsc::channel();
    let watcher: Box<dyn Watcher> = if RecommendedWatcher::kind() == WatcherKind::PollWatcher {
        Box::new(PollWatcher::with_delay(tx,Duration::from_secs(1)).unwrap())
    } else {
        Box::new(RecommendedWatcher::new(tx).unwrap())
    };
}