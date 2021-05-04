use std::{env, fs, thread, time::Duration};

extern crate notify;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc;

/// Test for <https://github.com/notify-rs/notify/issues/301>.
/// Note: This test will fail if your temp directory is not writable.
#[test]
fn test_race_with_remove_dir() {
    let tmpdir = env::temp_dir().join(".tmprPcUcB");
    fs::create_dir_all(&tmpdir).unwrap();

    {
        let tmpdir = tmpdir.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut watcher = RecommendedWatcher::new_raw(tx)
            .unwrap();

            watcher.watch(tmpdir, RecursiveMode::NonRecursive).unwrap();
        });
        thread::spawn(move || {
            for msg in rx {
                eprintln!("received event: {:?}", msg);
            }
        });
    }

    let subdir = tmpdir.join("146d921d.tmp");
    fs::create_dir_all(&subdir).unwrap();
    fs::remove_dir_all(&tmpdir).unwrap();
    thread::sleep(Duration::from_secs(1));
}
