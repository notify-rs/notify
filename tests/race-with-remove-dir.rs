use std::{fs, thread, time::Duration};

use notify::{immediate_watcher, RecursiveMode, Watcher};

/// Test for <https://github.com/notify-rs/notify/issues/301>.
/// Note: This test will fail if your temp directory is not writable.
#[test]
fn test_race_with_remove_dir() {
    let tmpdir = tempfile::tempdir().unwrap();

    {
        let tmpdir = tmpdir.path().to_path_buf();
        thread::spawn(move || {
            let mut watcher = immediate_watcher(move |result| {
                eprintln!("received event: {:?}", result);
            })
            .unwrap();

            watcher.watch(tmpdir, RecursiveMode::NonRecursive).unwrap();
        });
    }

    let subdir = tmpdir.path().join("146d921d.tmp");
    fs::create_dir_all(&subdir).unwrap();
    fs::remove_dir_all(&tmpdir).unwrap();
    thread::sleep(Duration::from_secs(1));
}
