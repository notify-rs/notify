use notify::{RecursiveMode, Result, Watcher, Config};
use std::path::Path;
fn direct_init() -> Result<()> {
    fn event_fn(res: Result<notify::Event>) {
        match res {
            Ok(event) => println!("event: {:?}", event),
            Err(e) => println!("watch error: {:?}", e),
        }
    }

    let mut watcher1 = notify::recommended_watcher(event_fn)?;
    // we will just use the same watcher kind again here
    let mut watcher2 = notify::recommended_watcher(event_fn)?;
    watcher1.watch(Path::new("."), RecursiveMode::Recursive)?;
    watcher2.watch(Path::new("."), RecursiveMode::Recursive)?;
    Ok(())
}

fn fallback_init() -> Result<()> {
    fn event_fn(res: Result<notify::Event>) {
        match res {
            Ok(event) => println!("event: {:?}", event),
            Err(e) => println!("watch error: {:?}", e),
        }
    }

    let mut watcher1 = notify::recommended_watcher_fallback(event_fn, Config::default())?;
    // we will just use the same watcher kind again here
    let mut watcher2 = notify::recommended_watcher_fallback(event_fn, Config::default())?;
    watcher1.watch(Path::new("."), RecursiveMode::Recursive)?;
    watcher2.watch(Path::new("."), RecursiveMode::Recursive)?;
    Ok(())
}

fn main() -> Result<()> {
    direct_init()?;
    fallback_init()?;
    Ok(())
}