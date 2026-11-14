# Upgrading from notify v8 to v9

This guide documents changes between v8 and v9 for upgrading existing code.
It also covers `notify-debouncer-full` 0.7 to 0.8, `notify-debouncer-mini` 0.7 to 0.8,
and changes made during the v9 release candidates.

Both debouncer 0.8 series use notify v9. If your application also depends on `notify`
directly, update that dependency together with the debouncer, or use the debouncer's
`notify` re-export so watcher, event, and configuration types come from the same version.

## Breaking changes

### 1) MSRV is now Rust 1.91

`notify` v9, both debouncer 0.8 series, and `notify-types` 2.3 require Rust 1.91 or newer.
These crates now use edition 2024; your application can keep its existing edition.

More constructors, builders, and getters now have `#[must_use]` annotations.
Consume their return values, or explicitly discard them with `let _ = ...` when intentional,
if your build treats unused-result warnings as errors.

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

Already applied operations are not rolled back. `remaining` does not include the failed
`origin`; handle that operation first if you intend to retry it, then retry `remaining`.
`origin` can be `None`, for example if restarting the backend fails after updating paths.
The conversion to `notify::Error` in the simple example above discards this recovery
information. Keep the `UpdatePathsError` when you need to inspect or retry the batch.

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

On Windows, the default separator style is inferred from the watched path: watching
`C:/src` produces `/` separators, while watching `C:\src` produces `\` separators.
If your application compares path strings and requires one style, set it explicitly:

```rust
use notify::{Config, WindowsPathSeparatorStyle};

let config = Config::default()
    .with_windows_path_separator_style(WindowsPathSeparatorStyle::Backslash);
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

### 6) Error construction and matching changed

`notify::Error` now contains a private field for per-watch user data.
Replace struct literals such as `Error { kind, paths }` with
`Error::new(kind).set_paths(paths)`, and add `..` when destructuring:
`let Error { kind, paths, .. } = error;`.

For code already using an earlier v9 RC, `UpdatePathsError` is now `#[non_exhaustive]`.
Construct it with `UpdatePathsError::new(source, origin, remaining)` and include `..`
when destructuring it.

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

### 7) Full debouncer changes

These changes apply to `notify-debouncer-full` 0.8, including upgrades from earlier
0.8 release candidates. `DebounceEventResult` remains
`Result<Vec<DebouncedEvent>, Vec<notify::Error>>`; the mini debouncer's batch enum does
not apply to the full debouncer.

#### Use the debouncer's methods directly

The deprecated `Debouncer::watcher()` and `Debouncer::cache()` methods have been removed.
They were already no-ops in 0.7, so remove remaining calls to them.
Use `debouncer.watch(...)`, `debouncer.unwatch(...)`, `debouncer.update_paths(...)`,
and `debouncer.configure(...)` directly. Root tracking is automatic; there is no separate
cache accessor for manually adding or removing roots.

#### Update custom file ID caches

Applications using `RecommendedCache`, `FileIdMap`, or `NoCache` do not need to implement
anything new. Custom `FileIdCache` implementations must account for these changes:

| API | Required change |
| --- | --- |
| `add_single_path(&mut self, path: &Path)` | Implement this new required method to cache only `path`, without walking its children. |
| `add_path(&mut self, path: &Path, recursive_mode: RecursiveMode)` | This now has a default tree walk that calls `add_single_path` for each entry. Remove your override if it only duplicates that walk. |
| `rescan(&mut self, roots: &[WatchRoot])` | If you override `rescan`, replace the old `&[(PathBuf, RecursiveMode)]` parameter and use `root.path` and `root.recursive_mode`. |

The default `add_path` follows symbolic links and includes the root and its descendants;
with `NonRecursive`, it includes only the root and its direct children.
The debouncer also calls `add_single_path` directly for events on children of non-recursive
watches. Implement it even if you keep a custom `add_path`, and do not delegate it to
`add_path(path, NonRecursive)`, which would also cache the child's children.

`WatchRoot` is non-exhaustive. Use its constructor instead of a struct literal and include
`..` when destructuring:

```rust
use notify_debouncer_full::{notify::RecursiveMode, WatchRoot};

let root = WatchRoot::new("src".into(), RecursiveMode::Recursive);
let WatchRoot { path, recursive_mode, .. } = root;
```

Unwatching a root now retains separately registered roots nested below it.
Custom caches can therefore receive `remove_path(parent)` followed by `add_path` calls
for surviving nested roots. To stop those watches too, unwatch them explicitly.

#### Handle removals without a preceding creation

A file created and removed within the debounce window now produces a `Remove` event;
previously the full debouncer could discard both events. A pending `Create` is replaced
by the removal. Applications maintaining a file index must tolerate a removal for a path
that is not in the index, rather than assuming every removal follows a delivered creation.

### 8) Cargo features and FreeBSD backend selection

The old `fsevent-sys` Cargo feature is no longer available. If your manifest enabled it
explicitly, use `macos_fsevent` instead. It remains enabled by default.
The FSEvents backend now uses `objc2-core-foundation` and `objc2-core-services` internally
(this was also backported to notify 8.2.1).

On FreeBSD, `RecommendedWatcher` now selects native inotify when built natively on
FreeBSD 14.5 or newer; otherwise it selects kqueue. Cross-compilation defaults to kqueue:
enable `freebsd_inotify` only when targeting FreeBSD 14.5 or newer. Both debouncers forward
this feature to notify. Code that needs kqueue behavior specifically can construct
`KqueueWatcher` explicitly instead of relying on `RecommendedWatcher`.

## Behavior changes to handle

### Watch errors and incomplete recursive coverage

- `PollWatcher::watch` now returns `ErrorKind::PathNotFound` for a missing path. Create the
  path before registering it, or watch an existing parent to detect its creation.
- On Windows, unwatching an unregistered path now returns `ErrorKind::WatchNotFound`.
  If your cleanup intentionally repeats `unwatch`, handle this error explicitly.
- On inotify and kqueue, `unwatch` preserves ancestor recursive coverage and returns
  `ErrorKind::WatchNotFound` for paths not explicitly registered.
  To ignore a subtree, filter its events in the event handler.
- Recursive inotify and kqueue watches can report recoverable child failures through the
  event handler while continuing to watch other entries. Handle callback errors as well as
  the return value of `watch`; `Ok(())` does not guarantee every descendant is covered.
  Watches installed before a later failure are not necessarily rolled back.
- `ErrorKind::MaxFilesWatch` also covers kqueue file descriptor limits. It is no longer
  specific to inotify; on kqueue, each watched file or directory uses a descriptor.
- Recursive kqueue watches skip special files such as FIFOs, sockets, and devices.
  Explicit watches on them return an `Unsupported` I/O error.

### Event details and rescan notifications

Windows now reports `CreateKind::File` or `CreateKind::Folder` when it can determine the
entry type. Match `EventKind::Create(_)` if you want all creations, rather than matching
only `CreateKind::Any`. Unknown kinds remain possible.

A non-recursive kqueue directory watch reports changes on the directory itself, without
identifying the affected child. It no longer attributes those notifications to arbitrary
existing children. Watch individual files or use a recursive watch when you need their changes.

FSEvents can share a stream root among overlapping recursive watches on the same volume.
Do not depend on receiving a separate root-change notification for every covered watch.

Handle `Event::need_rescan()` before filtering by path or event kind. Windows buffer
overflow and malformed notifications, FSEvents recovery, and inotify overflow can produce
rescan events, including events without paths. Refresh cached filesystem state directly;
a path or `user_data` value on the event does not establish that other watches are unaffected.
The full debouncer forwards the rescan flag on `DebouncedEvent`, while the mini debouncer
uses `DebouncedEventBatch::Rescan` as described above.

When inotify watches reach the same inode through symlinks or hard links, a change can now
be reported under each watch's path. Rename events for those watches can share a tracker.
If pairing raw `RenameMode::From` and `RenameMode::To` events yourself, match the pair
belonging to the same watch rather than treating the tracker as a unique event key.
The full debouncer handles this pairing by watch root.

## Non-breaking changes but worth mentioning

### Event-kind filtering was added

`Config::with_event_kinds` and `EventKindMask` allow filtering delivered events:

```rust
use notify::{Config, EventKindMask};

let config = Config::default().with_event_kinds(EventKindMask::CORE);
```

The default `Config` mask is `CORE | ACCESS_OPEN | ACCESS_CLOSE`, which reports the same event
kinds as v8, so no migration is required unless you want filtering.
`EventKindMask::default()` remains `ALL`; passing it explicitly also enables `ACCESS_CLOSE_NOWRITE`.

`EventKindMask::ALL` adds `ACCESS_CLOSE_NOWRITE`, which v8 never reported: inotify then emits
`Access(Close(Read))` when a file or directory opened without write access is closed.
Recursive registration itself generates directory access events: `Open` with the default mask,
and both `Open` and `Close(Read)` with `ALL`. Large walks periodically drain the inotify queue
and buffer these events until registration completes. Use `CORE` to avoid this event volume,
or `CORE | ACCESS_CLOSE` to keep close-after-write notifications.

### Per-watch settings and user data

`Watcher::watch_with` accepts a `WatchPathConfig`. For non-recursive inotify watches,
`with_dereference_symlinks(false)` watches a symbolic link itself, including a dangling
link. This is separate from `Config::with_follow_symlinks`, which controls traversal of
symbolic links within recursive watches.

`with_user_data(u64)` attaches an application-defined value to events and errors from a
watch on all built-in event-producing backends:

```rust
use notify::{RecursiveMode, Result, WatchPathConfig, Watcher};
use std::path::Path;

fn watch_with_id<W: Watcher>(watcher: &mut W, path: &Path, id: u64) -> Result<()> {
    watcher.watch_with(
        path,
        WatchPathConfig::new(RecursiveMode::Recursive).with_user_data(id),
    )
}
```

Read it with `Event::user_data()` or `Error::user_data()`. A failed batch watch exposes
it through `UpdatePathsError.source`. Rewatching replaces the value; using plain `watch`
resets it to `None`. For a rename, `From` carries the source watch's value, while `To`
and `Both` carry the destination watch's value. Values need not be unique.

For the full debouncer, pass per-watch settings through `update_paths`:

```rust
use notify_debouncer_full::{
    notify::{PathOp, RecursiveMode, UpdatePathsError, WatchPathConfig, Watcher},
    Debouncer, FileIdCache,
};
use std::path::PathBuf;

fn watch_with_id<W: Watcher, C: FileIdCache>(
    debouncer: &mut Debouncer<W, C>,
    path: PathBuf,
    id: u64,
) -> Result<(), UpdatePathsError> {
    debouncer.update_paths([PathOp::Watch(
        path,
        WatchPathConfig::new(RecursiveMode::Recursive).with_user_data(id),
    )])
}
```

`notify-types` 2.3 includes user data in event equality and hashing, so otherwise identical
events with different values compare unequal. Serde emits an optional `attrs.user_data`
field when configured; consumers enforcing a fixed JSON schema must allow that field.
Events without user data omit it, and older serialized events still deserialize.

### Custom watchers and additional configuration

`Watcher::watched_paths` returns registered paths and their recursive modes on built-in
backends. Its default implementation returns an error, so custom watchers must override
it to support listing. `watch_with` has a default implementation that applies the recursive
mode and tags returned errors; custom backends must override it to implement other per-watch
settings and attach user data to emitted events.

`PollWatcher::poll()` still queues a scan without waiting for it. Use the new
`poll_blocking()` when a caller needs to wait for that scan to finish, typically with
`Config::with_manual_polling()`. Do not call it from the poll watcher's own event handler,
which would wait for the thread currently running that handler.

`Config::with_windows_extended_information(true)` opts into more precise create/remove
event kinds on supported Windows APIs and filesystems. It is off by default. Unsupported
systems return errors instead of falling back automatically; use a default-configured
watcher for those paths. Both modes use 64 KiB buffers per OS watch.

`Config::with_fsevent_latency` controls FSEvents coalescing latency at construction time;
the default remains zero. This is separate from either debouncer's timeout.

The `futures` and `tokio` features add support for their unbounded channel senders as event
handlers in notify and both debouncers. Enable the corresponding feature on the crate
whose event handler you pass the sender to.
