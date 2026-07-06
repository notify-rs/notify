//! Black-box integration tests for `Watcher::watch_filtered` exercised through the public API.
//!
//! Unlike the per-backend unit tests, these drive the real, platform-default `RecommendedWatcher`
//! (inotify / FSEvents / kqueue / ReadDirectoryChangesW depending on the target) plus `PollWatcher`,
//! so they cover the config -> `update_paths` -> backend plumbing end to end. The two guarantees
//! asserted here hold on every backend:
//!
//! * events beneath a filter-rejected directory are suppressed, and
//! * events under a sibling included directory are still delivered.
//!
//! Watching a filter-rejected root and overlapping filtered watches are also checked, since those
//! errors come from shared code and must surface identically regardless of backend.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{
    recommended_watcher, Config, ErrorKind, Event, PollWatcher, RecursiveMode, Result, WatchFilter,
    Watcher,
};

/// A temporary directory whose path is canonicalized, matching what the event-time backends
/// report (macOS resolves `/var` -> `/private/var`; Windows returns a verbatim `\\?\` path).
struct TestDir {
    _tmp: tempfile::TempDir,
    path: PathBuf,
}

impl TestDir {
    fn path(&self) -> &Path {
        &self.path
    }
}

fn testdir() -> TestDir {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");
    TestDir { _tmp: tmp, path }
}

/// A filter that rejects directories named `name`.
fn reject_name(name: &'static str) -> WatchFilter {
    WatchFilter::with_filter(move |p: &Path| p.file_name() != Some(std::ffi::OsStr::new(name)))
}

fn drain(rx: &mpsc::Receiver<Result<Event>>) -> Vec<Event> {
    rx.try_iter()
        .map(|res| res.expect("watcher delivered an error"))
        .collect()
}

/// Drains events until one references `target` (its own path or something beneath it),
/// asserting throughout that nothing ever leaks from strictly inside `excluded`. Returns
/// whether `target` was observed before `deadline`.
///
/// The kqueue backend cannot tell *what* changed in a watched directory, so it rediscovers a
/// single new entry per write notification to the parent. Waiting for each sibling's own
/// creation before touching the next guarantees the backend gets a separate notification for
/// each and watches both; the other backends are unaffected by the extra synchronization.
fn wait_for_path(
    rx: &mpsc::Receiver<Result<Event>>,
    target: &Path,
    excluded: &Path,
    deadline: Instant,
) -> bool {
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(event)) => {
                assert!(
                    !event
                        .paths
                        .iter()
                        .any(|p| p.starts_with(excluded) && p.as_path() != excluded),
                    "event leaked from inside the excluded directory: {event:?}"
                );
                if event.paths.iter().any(|p| p.starts_with(target)) {
                    return true;
                }
            }
            Ok(Err(e)) => panic!("watcher delivered an error: {e:?}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("watcher channel disconnected"),
        }
    }
    false
}

/// FSEvents can transiently fail to start its stream under load; retry as the in-repo test
/// harness does. On the other backends the first attempt succeeds.
fn watch_filtered_retrying(watcher: &mut impl Watcher, root: &Path, filter: WatchFilter) {
    for attempt in 0..5 {
        match watcher.watch_filtered(root, RecursiveMode::Recursive, filter.clone()) {
            Ok(()) => return,
            Err(e) if attempt < 4 => {
                eprintln!("watch_filtered attempt {attempt} failed: {e:?}; retrying");
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => panic!("watch_filtered failed: {e:?}"),
        }
    }
}

fn assert_rejected_root_is_path_excluded(mut watcher: impl Watcher, root: &Path) {
    let rejecting = root.to_path_buf();
    let result = watcher.watch_filtered(
        root,
        RecursiveMode::Recursive,
        WatchFilter::with_filter(move |p: &Path| p != rejecting.as_path()),
    );
    assert!(
        matches!(result, Err(ref e) if matches!(e.kind, ErrorKind::PathExcluded)),
        "watching a filter-rejected root must fail with PathExcluded: {result:?}"
    );
}

#[test]
fn rejected_root_returns_path_excluded_recommended() {
    let dir = testdir();
    let (tx, _rx) = mpsc::channel();
    let watcher = recommended_watcher(tx).expect("recommended watcher");
    assert_rejected_root_is_path_excluded(watcher, dir.path());
}

#[test]
fn rejected_root_returns_path_excluded_poll() {
    let dir = testdir();
    let (tx, _rx) = mpsc::channel();
    let watcher = PollWatcher::new(tx, Config::default()).expect("poll watcher");
    assert_rejected_root_is_path_excluded(watcher, dir.path());
}

#[test]
fn overlapping_filtered_watches_are_refused() {
    let dir = testdir();
    let root = dir.path();
    let child = root.join("child");
    std::fs::create_dir(&child).expect("create child");

    let (tx, _rx) = mpsc::channel();
    let mut watcher = recommended_watcher(tx).expect("recommended watcher");
    watcher
        .watch_filtered(root, RecursiveMode::Recursive, reject_name("excluded"))
        .expect("first filtered watch");

    // A filtered directory watch may not overlap another watch in either direction.
    let result = watcher.watch_filtered(&child, RecursiveMode::Recursive, reject_name("other"));
    assert!(
        matches!(result, Err(ref e) if matches!(e.kind, ErrorKind::Generic(_))),
        "a filtered watch overlapping another must be refused: {result:?}"
    );
}

/// Deterministic (manually polled) end-to-end check that `PollWatcher` suppresses events beneath a
/// rejected directory while still delivering events from an included sibling.
#[test]
fn poll_watcher_suppresses_events_inside_excluded_directory() {
    let dir = testdir();
    let root = dir.path();
    let excluded = root.join("excluded");
    let included = root.join("included");
    std::fs::create_dir(&excluded).expect("create excluded");
    std::fs::create_dir(&included).expect("create included");

    let (tx, rx) = mpsc::channel();
    let mut watcher =
        PollWatcher::new(tx, Config::default().with_manual_polling()).expect("poll watcher");
    // The initial scan at watch time seeds the baseline (and prunes the excluded directory).
    watcher
        .watch_filtered(root, RecursiveMode::Recursive, reject_name("excluded"))
        .expect("watch filtered");
    let _ = drain(&rx);

    std::fs::write(excluded.join("hidden.txt"), "x").expect("write hidden");
    std::fs::write(included.join("seen.txt"), "x").expect("write seen");
    watcher.poll_blocking().expect("poll");

    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| e.paths.contains(&included.join("seen.txt"))),
        "expected an event for the file created under the included directory: {events:?}"
    );
    assert!(
        events.iter().all(|e| e
            .paths
            .iter()
            .all(|p| !p.starts_with(&excluded) || *p == excluded)),
        "no event may originate strictly inside the excluded directory: {events:?}"
    );
}

/// End-to-end check over the platform-default backend: writes beneath a rejected directory are
/// never delivered, while writes under an included sibling are. Timing-sensitive (real backends
/// deliver asynchronously), so it keeps writing until an included event arrives and asserts the
/// absence of excluded-subtree events throughout.
#[test]
fn recommended_watcher_suppresses_events_inside_excluded_directory() {
    let dir = testdir();
    let root = dir.path();

    let (tx, rx) = mpsc::channel();
    let mut watcher = recommended_watcher(tx).expect("recommended watcher");
    watch_filtered_retrying(&mut watcher, root, reject_name("excluded"));

    let excluded = root.join("excluded");
    let included = root.join("included");

    let deadline = Instant::now() + Duration::from_secs(15);

    // Create the siblings one at a time, waiting for each to be observed before creating the
    // next, so the kqueue backend gets a separate parent notification per directory and watches
    // `included` (see `wait_for_path`). Their own creation is delivered even though the filter
    // rejects `excluded` — only events strictly beneath a rejected directory are suppressed.
    std::fs::create_dir(&excluded).expect("create excluded");
    assert!(
        wait_for_path(&rx, &excluded, &excluded, deadline),
        "expected the excluded directory's own creation to be reported"
    );
    std::fs::create_dir(&included).expect("create included");
    assert!(
        wait_for_path(&rx, &included, &excluded, deadline),
        "expected the included directory's creation to be observed so it becomes watched"
    );

    let mut saw_included = false;
    while !saw_included && Instant::now() < deadline {
        std::fs::write(excluded.join("hidden.txt"), "x").expect("write hidden");
        std::fs::write(included.join("seen.txt"), "x").expect("write seen");

        let attempt_deadline = Instant::now() + Duration::from_millis(250);
        while !saw_included && Instant::now() < attempt_deadline {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(event)) => {
                    assert!(
                        !event
                            .paths
                            .iter()
                            .any(|p| p.starts_with(&excluded) && *p != excluded),
                        "event leaked from inside the excluded directory: {event:?}"
                    );
                    if event
                        .paths
                        .iter()
                        .any(|p| p.starts_with(&included) && *p != included)
                    {
                        saw_included = true;
                    }
                }
                Ok(Err(e)) => panic!("watcher delivered an error: {e:?}"),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => panic!("watcher channel disconnected"),
            }
        }
    }

    assert!(
        saw_included,
        "expected an event from inside the included directory within the deadline"
    );
}

/// The filter gates directories only, so watching a non-directory root is never rejected — even
/// when the filter would reject that exact path. Shared barrier logic, so check both backends.
fn assert_file_root_is_not_filtered(mut watcher: impl Watcher, file: &Path) {
    let rejecting = file.to_path_buf();
    let result = watcher.watch_filtered(
        file,
        RecursiveMode::Recursive,
        WatchFilter::with_filter(move |p: &Path| p != rejecting.as_path()),
    );
    assert!(
        result.is_ok(),
        "the filter gates directories only, so a file root must still be watched: {result:?}"
    );
}

#[test]
fn file_root_is_not_filtered_recommended() {
    let dir = testdir();
    let file = dir.path().join("watched.txt");
    std::fs::write(&file, "init").expect("create file");
    let (tx, _rx) = mpsc::channel();
    assert_file_root_is_not_filtered(recommended_watcher(tx).expect("recommended watcher"), &file);
}

#[test]
fn file_root_is_not_filtered_poll() {
    let dir = testdir();
    let file = dir.path().join("watched.txt");
    std::fs::write(&file, "init").expect("create file");
    let (tx, _rx) = mpsc::channel();
    let watcher = PollWatcher::new(tx, Config::default()).expect("poll watcher");
    assert_file_root_is_not_filtered(watcher, &file);
}

/// The filter gates directories only: a file whose name the filter would reject still produces
/// events under a recursive filtered watch. Deterministic via manual polling.
#[test]
fn filter_gates_directories_not_files() {
    let dir = testdir();
    let root = dir.path();

    let (tx, rx) = mpsc::channel();
    let mut watcher =
        PollWatcher::new(tx, Config::default().with_manual_polling()).expect("poll watcher");
    // `reject_name("seen.txt")` would reject a *directory* named seen.txt; a file is never gated.
    watcher
        .watch_filtered(root, RecursiveMode::Recursive, reject_name("seen.txt"))
        .expect("watch filtered");
    let _ = drain(&rx);

    let file = root.join("seen.txt");
    std::fs::write(&file, "data").expect("write file");
    watcher.poll_blocking().expect("poll");

    let events = drain(&rx);
    assert!(
        events.iter().any(|e| e.paths.contains(&file)),
        "the filter gates directories only; a matching file must still be reported: {events:?}"
    );
}

/// Re-watching the same path with an accept-all filter replaces the filtered watch and lifts the
/// exclusion, so contents that were previously suppressed start flowing. Deterministic.
#[test]
fn rewatching_with_accept_all_lifts_the_filter() {
    let dir = testdir();
    let root = dir.path();
    let excluded = root.join("excluded");
    std::fs::create_dir(&excluded).expect("create excluded");

    let (tx, rx) = mpsc::channel();
    let mut watcher =
        PollWatcher::new(tx, Config::default().with_manual_polling()).expect("poll watcher");
    watcher
        .watch_filtered(root, RecursiveMode::Recursive, reject_name("excluded"))
        .expect("watch filtered");
    let _ = drain(&rx);

    // With the filter in place, activity inside `excluded` is suppressed.
    std::fs::write(excluded.join("first.txt"), "a").expect("write first");
    watcher.poll_blocking().expect("poll");
    let events = drain(&rx);
    assert!(
        events.iter().all(|e| e
            .paths
            .iter()
            .all(|p| !p.starts_with(&excluded) || *p == excluded)),
        "the filter should suppress excluded contents: {events:?}"
    );

    // Re-watching the same path without a filter replaces the watch and lifts the exclusion.
    watcher
        .watch(root, RecursiveMode::Recursive)
        .expect("rewatch accept-all");
    let _ = drain(&rx);

    std::fs::write(excluded.join("second.txt"), "b").expect("write second");
    watcher.poll_blocking().expect("poll");
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| e.paths.contains(&excluded.join("second.txt"))),
        "after re-watching without a filter, excluded contents must be reported: {events:?}"
    );
}
