# Upgrading from notify v8 to v9

This guide documents changes between v8 and v9 for upgrading existing code.

## Breaking changes

### 1) MSRV is now Rust 1.85

`notify` v9 requires Rust 1.85 or newer.

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
