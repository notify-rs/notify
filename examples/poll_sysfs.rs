/// Example for watching kernel internal filesystems like `/sys` and `/proc`
/// These can't be watched by the default backend or unconfigured pollwatcher
/// This example can't be demonstrated under windows, it might be relevant for network shares
#[cfg(not(target_os = "windows"))]
fn not_windows_main() -> notify::Result<()> {
    use notify::{PollWatcher, RecursiveMode, Watcher, Config};
    use std::path::Path;
    use std::time::Duration;

    let mut paths: Vec<_> = std::env::args()
        .skip(1)
        .map(|arg| Path::new(&arg).to_path_buf())
        .collect();
    if paths.is_empty() {
        let lo_stats = Path::new("/sys/class/net/lo/statistics/tx_bytes").to_path_buf();
        if !lo_stats.exists() {
            eprintln!("Must provide path to watch, default system path was not found (probably you're not running on Linux?)");
            std::process::exit(1);
        }
        println!(
            "Trying {:?}, use `ping localhost` to see changes!",
            lo_stats
        );
        paths.push(lo_stats);
    }

    println!("watching {:?}...", paths);
    // configure pollwatcher backend
    let config = Config::default()
        .with_compare_contents(true) // crucial part for pseudo filesystems 
        .with_poll_interval(Duration::from_secs(2));
    let (tx, rx) = std::sync::mpsc::channel();
    // create pollwatcher backend
    let mut watcher = PollWatcher::new(tx, config)?;
    for path in paths {
        // watch all paths
        watcher.watch(&path, RecursiveMode::Recursive)?;
    }
    // print all events, never returns
    for res in rx {
        match res {
            Ok(event) => println!("changed: {:?}", event),
            Err(e) => println!("watch error: {:?}", e),
        }
    }

    Ok(())
}

fn main() -> notify::Result<()> {
    #[cfg(not(target_os = "windows"))]
    {
        not_windows_main()
    }
    #[cfg(target_os = "windows")]
    notify::Result::Ok(())
}
