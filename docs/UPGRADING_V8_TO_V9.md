# Upgrading from notify v8 to v9

This guide documents changes between v8 and v9 for upgrading existing code.

## Breaking changes

### 1) MSRV is now Rust 1.88

`notify` v9 requires Rust 1.88 or newer.

We also declared a new MSRV policy. For details, please read README.

### 2) `Watcher::paths_mut` was removed

`PathsMut` and `Watcher::paths_mut()` were removed in v9 and replaced by:

- `Watcher::update_paths(Vec<PathOp>)`
- `PathOp` and `WatchPathConfig`
- `UpdatePathsError` for partial-failure reporting

Before (v8):

```rust
use notify::{RecursiveMode, Result, Watcher};

fn add_many_paths<W: Watcher>(watcher: &mut W, paths: &[std::path::PathBuf]) -> Result<()> {
    let mut batch = watcher.paths_mut();
    for path in paths {
        batch.add(path, RecursiveMode::Recursive)?;
    }
    batch.commit()
}
```

After (v9):

```rust
use notify::{PathOp, Result, Watcher};

fn add_many_paths<W: Watcher>(watcher: &mut W, paths: &[std::path::PathBuf]) -> Result<()> {
    let ops = paths
        .iter()
        .cloned()
        .map(PathOp::watch_recursive)
        .collect::<Vec<_>>();

    watcher.update_paths(ops).map_err(notify::Error::from)?;
    Ok(())
}
```

`update_paths` applies operations in order and stops on the first error.
When it fails, `UpdatePathsError` includes:

- `source`: underlying `notify::Error`
- `origin`: failing operation (if known)
- `remaining`: operations that were not attempted

This lets you retry only unfinished operations if needed.

If you implemented a custom watcher and overrode `paths_mut`, migrate that logic to `update_paths`.

### 3) Event paths preserve the watched path representation

`Event.paths` and `Watcher::watched_paths()` now use the same root representation that was passed
to `Watcher::watch` or `Watcher::update_paths`.

For example, watching `src` now reports `src/lib.rs`. Watching `/repo/src` reports
`/repo/src/lib.rs`.

If your code relied on Linux or Windows backends converting relative watch paths to absolute event
paths, convert the path before calling `watch`:

```rust
let path = std::env::current_dir()?.join("src");
watcher.watch(&path, notify::RecursiveMode::Recursive)?;
```

### 4) Rewatching the same path replaces the existing watch

Calling `Watcher::watch` again for the same backend-resolved path now replaces the existing watch
on success. The recursive mode and reported path are updated to the new request, a second
independent watch is not added, and one `Watcher::unwatch` call removes the path.

In v8 this behavior varied by backend. Some backends effectively merged repeated watches, while
others could keep duplicate backend entries or resources. If your code depended on repeated
`watch` calls acting like independent watches for the same path, use separate watcher instances or
manage parent/child watches explicitly.

Before (v8 behavior varied by backend):

```rust
watcher.watch(path, notify::RecursiveMode::Recursive)?;
watcher.watch(path, notify::RecursiveMode::NonRecursive)?;
```

After (v9):

```rust
watcher.watch(path, notify::RecursiveMode::Recursive)?;
watcher.watch(path, notify::RecursiveMode::NonRecursive)?;

// `path` is now watched non-recursively.
watcher.unwatch(path)?;
```

### 5) Mini debouncer notifications

`notify-debouncer-mini` 0.8 uses notify v9. Its `DebounceEventResult` is now
`Result<DebouncedEventBatch, notify::Error>` instead of `Result<Vec<DebouncedEvent>, notify::Error>`.
Replace the `Ok(events)` branch with `Ok(DebouncedEventBatch::Events(events))` and handle the
new `Rescan` variant:

```rust
use notify_debouncer_mini::{DebounceEventResult, DebouncedEventBatch};

fn handle_event(result: DebounceEventResult) {
    match result {
        Ok(DebouncedEventBatch::Events(events)) => {
            for event in events {
                println!("Event {:?} for {:?}", event.kind, event.path);
            }
        }
        Ok(DebouncedEventBatch::Rescan) => {
            // Refresh any cached state for all watched paths here.
            println!("Events were missed; rescan all watched paths");
        }
        Err(error) => eprintln!("Watch error: {error}"),
    }
}
```

Previously, the mini debouncer discarded rescan flags. It now forwards them without waiting for
the debounce timeout, including when the backend supplies no paths. `Rescan` applies to all
watched paths regardless of any paths on the original event, so handle it before applying path
or event-kind filters. Pending path changes keep their usual debounce deadlines, and errors
are still delivered immediately.

The existing `DebouncedEvent` and `DebouncedEventKind` types are unchanged. `DebouncedEventBatch`
is an additive API in `notify-types` 2.2; the breaking change is in `notify-debouncer-mini`.

### 6) `ErrorKind` is now non-exhaustive

`notify::ErrorKind` is now marked `#[non_exhaustive]` so future releases can add error kinds.
If your v8 code matched every variant in `match err.kind { ... }`, add a wildcard arm (`_`).
It is required even when all currently defined variants are handled.

Keep any existing variant-specific handling and provide a fallback for other error kinds:

```rust
use notify::{Error, ErrorKind};

fn handle_error(err: Error) {
    match &err.kind {
        ErrorKind::PathNotFound => eprintln!("Watched path not found: {:?}", err.paths),
        _ => eprintln!("Watch error: {err}"),
    }
}
```

## Non-breaking changes but worth mentioning

### Event-kind filtering was added

`Config::with_event_kinds` and `EventKindMask` allow filtering delivered events:

```rust
use notify::{Config, EventKindMask};

let config = Config::default().with_event_kinds(EventKindMask::CORE);
```

No migration is required unless you want filtering.

### macOS FSEvents backend internals changed

The `macos_fsevent` feature now uses `objc2-core-foundation` and `objc2-core-services` instead of `fsevent-sys`.
Public `notify` API stays the same, but macOS behavior should be revalidated in integration tests.
