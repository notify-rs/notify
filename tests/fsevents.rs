#![allow(dead_code)]

extern crate notify;
extern crate tempdir;
extern crate time;

mod utils;

#[cfg(all(target_os = "macos", feature = "timing_tests"))]
mod timing_tests {
    use notify::*;
    use std::sync::mpsc;
    use tempdir::TempDir;

    use utils::*;

    const TIMEOUT_S: f64 = 5.0;

    #[test]
    fn fsevents_create_delete_file_0() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.create("file1");
        tdir.remove("file1");

        let actual = recv_events_with_timeout(&rx, TIMEOUT_S);
        if actual == vec![(tdir.mkpath("file1"), op::CREATE | op::REMOVE, None)] {
            panic!("single CREATE | REMOVE event");
        } else {
            assert_eq!(actual, vec![
                (tdir.mkpath("file1"), op::CREATE, None),
                (tdir.mkpath("file1"), op::CREATE | op::REMOVE, None),
            ]);
        }
    }

    #[test]
    fn fsevents_create_delete_file_1() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.create("file1");

        assert_eq!(recv_events_with_timeout(&rx, 1.0), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
        ]);

        tdir.remove("file1");

        let actual = recv_events_with_timeout(&rx, TIMEOUT_S);
        if actual == vec![(tdir.mkpath("file1"), op::CREATE | op::REMOVE, None)] {
            panic!("excessive CREATE event");
        } else {
            assert_eq!(actual, vec![
                (tdir.mkpath("file1"), op::REMOVE, None),
            ]);
        }
    }

    #[test]
    fn fsevents_create_delete_file_2() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.create("file1");

        assert_eq!(recv_events_with_timeout(&rx, 2.0), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
        ]);

        tdir.remove("file1");

        let actual = recv_events_with_timeout(&rx, TIMEOUT_S);
        if actual == vec![(tdir.mkpath("file1"), op::CREATE | op::REMOVE, None)] {
            panic!("excessive CREATE event");
        } else {
            assert_eq!(actual, vec![
                (tdir.mkpath("file1"), op::REMOVE, None),
            ]);
        }
    }

    #[test]
    fn fsevents_create_delete_file_4() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.create("file1");

        assert_eq!(recv_events_with_timeout(&rx, 4.0), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
        ]);

        tdir.remove("file1");

        let actual = recv_events_with_timeout(&rx, TIMEOUT_S);
        if actual == vec![(tdir.mkpath("file1"), op::CREATE | op::REMOVE, None)] {
            panic!("excessive CREATE event");
        } else {
            assert_eq!(actual, vec![
                (tdir.mkpath("file1"), op::REMOVE, None),
            ]);
        }
    }

    #[test]
    fn fsevents_create_delete_file_8() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.create("file1");

        assert_eq!(recv_events_with_timeout(&rx, 8.0), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
        ]);

        tdir.remove("file1");

        let actual = recv_events_with_timeout(&rx, TIMEOUT_S);
        if actual == vec![(tdir.mkpath("file1"), op::CREATE | op::REMOVE, None)] {
            panic!("excessive CREATE event");
        } else {
            assert_eq!(actual, vec![
                (tdir.mkpath("file1"), op::REMOVE, None),
            ]);
        }
    }

    #[test]
    fn fsevents_create_delete_file_16() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.create("file1");

        assert_eq!(recv_events_with_timeout(&rx, 16.0), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
        ]);

        tdir.remove("file1");

        let actual = recv_events_with_timeout(&rx, TIMEOUT_S);
        if actual == vec![(tdir.mkpath("file1"), op::CREATE | op::REMOVE, None)] {
            panic!("excessive CREATE event");
        } else {
            assert_eq!(actual, vec![
                (tdir.mkpath("file1"), op::REMOVE, None),
            ]);
        }
    }

    #[test]
    fn fsevents_create_delete_file_32() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.create("file1");

        assert_eq!(recv_events_with_timeout(&rx, 32.0), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
        ]);

        tdir.remove("file1");

        let actual = recv_events_with_timeout(&rx, TIMEOUT_S);
        if actual == vec![(tdir.mkpath("file1"), op::CREATE | op::REMOVE, None)] {
            panic!("excessive CREATE event");
        } else {
            assert_eq!(actual, vec![
                (tdir.mkpath("file1"), op::REMOVE, None),
            ]);
        }
    }

    #[test]
    fn fsevents_create_delete_file_64() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.create("file1");

        assert_eq!(recv_events_with_timeout(&rx, 64.0), vec![
            (tdir.mkpath("file1"), op::CREATE, None),
        ]);

        tdir.remove("file1");

        let actual = recv_events_with_timeout(&rx, TIMEOUT_S);
        if actual == vec![(tdir.mkpath("file1"), op::CREATE | op::REMOVE, None)] {
            panic!("excessive CREATE event");
        } else {
            assert_eq!(actual, vec![
                (tdir.mkpath("file1"), op::REMOVE, None),
            ]);
        }
    }

    #[test]
    fn fsevents_rename_rename_file_0() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        tdir.create_all(vec![
            "file1a",
        ]);

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.rename("file1a", "file1b");
        tdir.rename("file1b", "file1c");

        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, None),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1c"), op::RENAME, Some(cookies[0])),
        ]);
    }

    #[test]
    fn fsevents_rename_rename_file_10() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        tdir.create_all(vec![
            "file1a",
        ]);

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.rename("file1a", "file1b");
        sleep(10);
        tdir.rename("file1b", "file1c");

        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[1])),
            (tdir.mkpath("file1c"), op::RENAME, Some(cookies[1])),
        ]);
    }

    #[test]
    fn fsevents_rename_rename_file_20() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        tdir.create_all(vec![
            "file1a",
        ]);

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.rename("file1a", "file1b");
        sleep(20);
        tdir.rename("file1b", "file1c");

        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[1])),
            (tdir.mkpath("file1c"), op::RENAME, Some(cookies[1])),
        ]);
    }

    #[test]
    fn fsevents_rename_rename_back_file_0() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        tdir.create_all(vec![
            "file1a",
        ]);

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.rename("file1a", "file1b");
        tdir.rename("file1b", "file1a");

        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 1);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, Some(cookies[0])),
        ]);
    }

    #[test]
    fn fsevents_rename_rename_back_file_10() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        tdir.create_all(vec![
            "file1a",
        ]);

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.rename("file1a", "file1b");
        sleep(10);
        tdir.rename("file1b", "file1a");

        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[1])),
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, Some(cookies[1])),
        ]);
    }

    #[test]
    fn fsevents_rename_rename_back_file_20() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        tdir.create_all(vec![
            "file1a",
        ]);

        sleep(10);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.rename("file1a", "file1b");
        sleep(20);
        tdir.rename("file1b", "file1a");

        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[1])),
            (tdir.mkpath("file1a"), op::CREATE | op::RENAME, Some(cookies[1])),
        ]);
    }

    #[test]
    fn fsevents_rename_rename_back_file_sleep() {
        let tdir = TempDir::new("temp_dir").expect("failed to create temporary directory");

        tdir.create_all(vec![
            "file1a",
        ]);

        sleep(40_000);

        let (tx, rx) = mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).expect("failed to create recommended watcher");
        watcher.watch(tdir.mkpath("."), RecursiveMode::Recursive).expect("failed to watch directory");

        tdir.rename("file1a", "file1b");
        sleep(10);
        tdir.rename("file1b", "file1a");

        let actual = recv_events(&rx);
        let cookies = extract_cookies(&actual);
        assert_eq!(cookies.len(), 2);
        assert_eq!(actual, vec![
            (tdir.mkpath("file1a"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[0])),
            (tdir.mkpath("file1b"), op::RENAME, Some(cookies[1])),
            (tdir.mkpath("file1a"), op::RENAME, Some(cookies[1])),
        ]);
    }
}
