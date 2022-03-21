#![cfg(not(target_os = "windows"))]

use std::path::Path;
use std::time::Duration;
use notify::poll::PollWatcherConfig;
use notify::{PollWatcher, RecursiveMode, Watcher};

fn main() -> notify::Result<()> {
  let mut paths: Vec<_> = std::env::args().skip(1)
      .map(|arg| Path::new(&arg).to_path_buf())
      .collect();
  if paths.is_empty() {
    let lo_stats = Path::new("/sys/class/net/lo/statistics/tx_bytes").to_path_buf();
    if !lo_stats.exists() {
      eprintln!("Must provide path to watch, default system path was not found (probably you're not running on Linux?)");
      std::process::exit(1);
    }
    println!("Trying {:?}, use `ping localhost` to see changes!", lo_stats);
    paths.push(lo_stats);
  }

  println!("watching {:?}...", paths);

  let config = PollWatcherConfig {
    compare_contents: true,
    poll_interval: Duration::from_secs(2),
  };
  let (tx, rx) = std::sync::mpsc::channel();
  let mut watcher = PollWatcher::with_config(tx, config)?;
  for path in paths {
    watcher.watch(&path, RecursiveMode::Recursive)?;
  }

  for res in rx {
    match res {
      Ok(event) => println!("changed: {:?}", event),
      Err(e) => println!("watch error: {:?}", e),
    }
  }

  Ok(())
}
