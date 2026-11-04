use notify::{
    Config, Event, PathOp, PollWatcher, RecommendedWatcher, RecursiveMode, WatchPathConfig, Watcher,
};
use std::{
    fs,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

fn wait_for(
    rx: &mpsc::Receiver<notify::Result<Event>>,
    path: &Path,
    kind: impl Fn(&Event) -> bool,
) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let event = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|e| panic!("waiting for {path:?}: {e}"))
            .unwrap();
        if event.paths == [path] && kind(&event) {
            return event;
        }
    }
}

fn exercise_user_data<W: Watcher>() {
    let dir = tempfile::tempdir().unwrap();
    let left = dir.path().join("left");
    let right = dir.path().join("right");
    fs::create_dir_all(left.join("existing")).unwrap();
    fs::create_dir(&right).unwrap();
    let (tx, rx) = mpsc::channel();
    let mut watcher = W::new(
        tx,
        Config::default()
            .with_poll_interval(Duration::from_millis(10))
            .with_compare_contents(true),
    )
    .unwrap();
    watcher
        .update_paths(vec![
            PathOp::Watch(
                left.clone(),
                WatchPathConfig::new(RecursiveMode::Recursive).with_user_data(0),
            ),
            PathOp::Watch(
                right.clone(),
                WatchPathConfig::new(RecursiveMode::Recursive).with_user_data(u64::MAX),
            ),
        ])
        .unwrap();

    for (root, value) in [(left.join("existing"), 0), (right.clone(), u64::MAX)] {
        let file = root.join("file");
        fs::write(&file, "first").unwrap();
        assert_eq!(
            wait_for(&rx, &file, |e| e.kind.is_create()).user_data(),
            Some(value)
        );
        fs::write(&file, "second").unwrap();
        assert_eq!(
            wait_for(&rx, &file, |e| e.kind.is_modify()).user_data(),
            Some(value)
        );
        fs::remove_file(&file).unwrap();
        assert_eq!(
            wait_for(&rx, &file, |e| e.kind.is_remove()).user_data(),
            Some(value)
        );
    }

    let added = left.join("added");
    fs::create_dir(&added).unwrap();
    assert_eq!(
        wait_for(&rx, &added, |e| e.kind.is_create()).user_data(),
        Some(0)
    );
    // For inotify, wait until the event loop has finished installing descendant watches.
    watcher.watched_paths().unwrap();
    let file = added.join("file");
    fs::write(&file, "content").unwrap();
    assert_eq!(
        wait_for(&rx, &file, |e| e.kind.is_create()).user_data(),
        Some(0)
    );

    watcher
        .watch_with(
            &left,
            WatchPathConfig::new(RecursiveMode::Recursive).with_user_data(42),
        )
        .unwrap();
    let file = added.join("updated");
    fs::write(&file, "content").unwrap();
    assert_eq!(
        wait_for(&rx, &file, |e| e.kind.is_create()).user_data(),
        Some(42)
    );

    watcher.watch(&left, RecursiveMode::Recursive).unwrap();
    let file = added.join("reset");
    fs::write(&file, "content").unwrap();
    assert_eq!(
        wait_for(&rx, &file, |e| e.kind.is_create()).user_data(),
        None
    );

    let file = right.join("unchanged");
    fs::write(&file, "content").unwrap();
    assert_eq!(
        wait_for(&rx, &file, |e| e.kind.is_create()).user_data(),
        Some(u64::MAX)
    );

    let file = dir.path().join("single-file");
    fs::write(&file, "before").unwrap();
    watcher
        .watch_with(
            &file,
            WatchPathConfig::new(RecursiveMode::NonRecursive).with_user_data(7),
        )
        .unwrap();
    fs::write(&file, "after").unwrap();
    assert_eq!(
        wait_for(&rx, &file, |e| e.kind.is_modify()).user_data(),
        Some(7)
    );
}

fn exercise_overlapping_user_data<W: Watcher>() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let (tx, rx) = mpsc::channel();
    let mut watcher = W::new(
        tx,
        Config::default().with_poll_interval(Duration::from_millis(10)),
    )
    .unwrap();
    for (path, id) in [(dir.path(), 1), (nested.as_path(), 2)] {
        watcher
            .watch_with(
                path,
                WatchPathConfig::new(RecursiveMode::Recursive).with_user_data(id),
            )
            .unwrap();
    }
    let file = nested.join("file");
    fs::write(&file, "content").unwrap();
    let mut events = std::collections::HashSet::new();
    for _ in 0..2 {
        events.insert(wait_for(&rx, &file, |e| e.kind.is_create()));
    }
    assert_eq!(events.len(), 2);
    assert_eq!(
        events
            .iter()
            .filter_map(Event::user_data)
            .collect::<std::collections::BTreeSet<_>>(),
        [1, 2].into_iter().collect()
    );
}

fn exercise_error_user_data<W: Watcher>() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let mut watcher = W::new(|_| {}, Config::default().with_manual_polling()).unwrap();
    for value in [None, Some(0), Some(u64::MAX)] {
        let mut config = WatchPathConfig::new(RecursiveMode::Recursive);
        if let Some(value) = value {
            config = config.with_user_data(value);
        }
        let error = watcher.watch_with(&missing, config.clone()).unwrap_err();
        assert_eq!(error.user_data(), value);

        let error = watcher
            .update_paths(vec![
                PathOp::Watch(missing.clone(), config),
                PathOp::watch_recursive(dir.path()),
            ])
            .unwrap_err();
        assert_eq!(error.source.user_data(), value);
        assert!(matches!(&error.origin, Some(PathOp::Watch(path, _)) if path == &missing));
        assert_eq!(error.remaining.len(), 1);
        assert!(watcher.watched_paths().unwrap().is_empty());
    }
    assert_eq!(
        watcher
            .watch(&missing, RecursiveMode::Recursive)
            .unwrap_err()
            .user_data(),
        None
    );
}

#[test]
fn recommended_watcher_error_user_data() {
    exercise_error_user_data::<RecommendedWatcher>();
}

#[test]
fn poll_watcher_error_user_data() {
    exercise_error_user_data::<PollWatcher>();
}

#[test]
fn poll_scan_errors_keep_each_watch_value() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("watched");
    let child = root.join("child");
    fs::create_dir_all(&child).unwrap();
    let (tx, rx) = mpsc::channel();
    let mut watcher = PollWatcher::new(tx, Config::default().with_manual_polling()).unwrap();
    for (path, value) in [(&root, 0), (&child, u64::MAX)] {
        watcher
            .watch_with(
                path,
                WatchPathConfig::new(RecursiveMode::Recursive).with_user_data(value),
            )
            .unwrap();
    }
    fs::remove_dir_all(&root).unwrap();
    watcher.poll_blocking().unwrap();
    let mut errors: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
    errors.sort_by_key(|error| error.user_data());
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0].user_data(), Some(0));
    assert_eq!(errors[0].paths, [root]);
    assert_eq!(errors[1].user_data(), Some(u64::MAX));
    assert_eq!(errors[1].paths, [child]);
}

#[cfg(unix)]
#[test]
fn poll_initial_and_periodic_loop_errors_keep_user_data() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("loop")).unwrap();
    let (tx, rx) = mpsc::channel();
    let mut watcher = PollWatcher::new(tx, Config::default().with_manual_polling()).unwrap();
    watcher
        .watch_with(
            dir.path(),
            WatchPathConfig::new(RecursiveMode::Recursive).with_user_data(42),
        )
        .unwrap();
    let check = || {
        let errors: Vec<_> = rx.try_iter().filter_map(Result::err).collect();
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0].kind, notify::ErrorKind::Generic(_)));
        assert_eq!(errors[0].user_data(), Some(42));
    };
    check();
    watcher.poll_blocking().unwrap();
    check();
}

#[test]
fn recommended_watcher_user_data() {
    exercise_user_data::<RecommendedWatcher>();
    #[cfg(windows)]
    exercise_overlapping_user_data::<RecommendedWatcher>();
}

#[test]
fn poll_watcher_user_data() {
    exercise_user_data::<PollWatcher>();
    exercise_overlapping_user_data::<PollWatcher>();
}
