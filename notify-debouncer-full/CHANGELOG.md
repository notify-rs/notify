# Changelog

## unreleased

## debouncer-full 0.8.0 (2026-11-21)

This release uses notify 9.0.0 and requires Rust 1.91. For examples of the updated APIs, see the [0.7 to 0.8 migration guide](../docs/UPGRADING_V8_TO_V9.md#7-full-debouncer-changes).

- Pass a `Config` to `new_debouncer_opt` and call watch methods directly on `Debouncer`.
- Custom `FileIdCache` implementations must provide `add_single_path` and use `&[WatchRoot]` when overriding `rescan`.
- The default event filter excludes access events.
- Files created and removed within a debounce window now produce a `Remove` event.

### Changes since 0.8.0-rc.4

- FIX: emit renames before events that reuse their source paths, fixing Blender's backup saves [#587]
- FIX: report source removals when renamed files or their parent directories are removed or replaced before delivery
- FIX: preserve watch `user_data` in synthesized `Remove` events
- PERF: reduce allocations during event delivery and avoid repeatedly draining queues blocked by renames

[#587]: https://github.com/notify-rs/notify/issues/587

## debouncer-full 0.8.0-rc.4 (2026-11-15)

- CHANGE: upgrade `notify` to 9.0.0-rc.9 and `notify-types` to 2.3.1
- CHANGE: `new_debouncer_opt` takes `(Config, event_handler, file_id_cache)` instead of five arguments. See the [migration guide](../docs/UPGRADING_V8_TO_V9.md#pass-a-config-to-new_debouncer_opt) [#1106] **breaking**
- CHANGE: `Debouncer::watched_paths` returns `Vec<notify::WatchedPath>` instead of `Vec<(PathBuf, RecursiveMode)>` [#1106] **breaking**
- CHANGE: `new_debouncer` and `Config::default()` use `EventKindMask::CORE`, excluding access events. See the [migration guide](../docs/UPGRADING_V8_TO_V9.md#event-kind-filtering-was-added) [#1103]

[#1103]: https://github.com/notify-rs/notify/pull/1103
[#1106]: https://github.com/notify-rs/notify/pull/1106

## debouncer-full 0.8.0-rc.3 (2026-11-09)

- CHANGE: upgrade `notify` to 9.0.0-rc.8 and `notify-types` to 2.3.0
- CHANGE: raise MSRV from 1.88 to 1.91
- CHANGE: update to edition 2024 [#990]
- CHANGE: remove the deprecated no-op methods `Debouncer::watcher` and `Debouncer::cache` [#1059] **breaking**
- CHANGE: `FileIdCache::add_single_path` is a required method, and `FileIdCache::add_path` is a provided method that walks the path and calls `add_single_path` for each entry; a custom cache only has to cache single paths [#1034] **breaking**
- CHANGE: `FileIdCache::rescan` takes `&[WatchRoot]` instead of `&[(PathBuf, RecursiveMode)]`; `WatchRoot` is `#[non_exhaustive]`, provides `WatchRoot::new`, and requires `..` when destructuring [#1027] [#1060] **breaking**
- FEATURE: add the `freebsd_inotify` feature to enable notify's native inotify backend when cross-compiling for FreeBSD 14.5+ [#972] [#989]
- PERF: park the debouncer thread to avoid idle polling, and only wake it when work arrives while it is idle [#933] [#1078]
- PERF: speed up file ID lookups and registration with a hash map and an index of child paths for subtree removal [#1078]
- PERF: avoid scanning unrelated event queues and cached file IDs when removing a path [#1073]
- FIX: for an event on a direct child of a non-recursive watch, cache only that entry and not the entries below it, which the watch does not report [#1037]
- FIX: pair the source and the destination of a rename below the same root when the watcher reports the rename once for each watch, for example for two watches that reach one directory through a symlink [#1054]
- FIX: pair the source and the destination of a rename by file ID only when one of them has no tracker, so a later move-in is no longer reported as a rename of a file that an earlier rename moved [#1070]
- FIX: unwatching a path no longer deregisters roots nested under it, and re-fingerprints the nested roots whose cached IDs the removal dropped [#1028]
- FIX: if the watcher fails to construct, `new_debouncer_opt` no longer leaks the debouncer thread or the event handler [#1040]

[#933]: https://github.com/notify-rs/notify/pull/933
[#972]: https://github.com/notify-rs/notify/pull/972
[#989]: https://github.com/notify-rs/notify/pull/989
[#990]: https://github.com/notify-rs/notify/pull/990
[#1027]: https://github.com/notify-rs/notify/pull/1027
[#1028]: https://github.com/notify-rs/notify/pull/1028
[#1034]: https://github.com/notify-rs/notify/pull/1034
[#1037]: https://github.com/notify-rs/notify/pull/1037
[#1040]: https://github.com/notify-rs/notify/issues/1040
[#1054]: https://github.com/notify-rs/notify/pull/1054
[#1059]: https://github.com/notify-rs/notify/pull/1059
[#1060]: https://github.com/notify-rs/notify/pull/1060
[#1070]: https://github.com/notify-rs/notify/pull/1070
[#1073]: https://github.com/notify-rs/notify/pull/1073
[#1078]: https://github.com/notify-rs/notify/pull/1078

## debouncer-full 0.8.0-rc.2 (2026-05-02)

- CHANGE: upgrade `notify` to 9.0.0-rc.4
- CHANGE: emit `remove` events even if a file was created and then removed (because macOS repeats the "create" event) [#900] **breaking**
- CHANGE: speed up debouncer root tracking for large numbers of watched paths while preserving recursive matching for overlapping roots [#913]

[#900]: https://github.com/notify-rs/notify/issues/900
[#913]: https://github.com/notify-rs/notify/pull/913

## debouncer-full 0.8.0-rc.1 (2026-04-16)

- CHANGE: raise MSRV to 1.88
- CHANGE: upgrade `notify` to 9.0.0-rc.3
- CHANGE: add `#[must_use]` annotations to cache constructors and watcher kind accessors
- CHANGE: use `HashMap::extract_if` to reduce debouncer flush overhead
- CHANGE: speed up debouncer event flushing and file ID cache lookups while preserving stable path ordering for equal-timestamp events
- FEATURE: impl `EventHandler` for `futures::channel::mpsc::UnboundedSender` and `tokio::sync::mpsc::UnboundedSender` behind the `futures` and `tokio` feature flags [#767]
- FEATURE: add support of a watcher's method `update_paths`  [#705]
- FEATURE: add `Debouncer::watched_paths` for `Debouncer` [#710]

[#767]: https://github.com/notify-rs/notify/pull/767
[#705]: https://github.com/notify-rs/notify/pull/705
[#710]: https://github.com/notify-rs/notify/issues/710

## debouncer-full 0.7.0 (2026-01-23)

> [!IMPORTANT]
> The MSRV policy has been changed since this release.
> Check out README for details.

- FEATURE: support wasm build [#673]
- FIX: events within the timeout were not deduplicated, causing `event_handler` to be called multiple times for events that should have been merged [#711]

[#673]: https://github.com/notify-rs/notify/pull/673
[#711]: https://github.com/notify-rs/notify/pull/711

## debouncer-full 0.6.0 (2025-08-03)
- FEATURE: allow `FileIdCache` trait implementations to choose ownership of the returned file-ids [#664]
- FEATURE: added support for the [`flume`](https://docs.rs/flume) crate [#680]
- FIX: skip all `Modify` events right after a `Create` event, unless it's a rename event [#701]

[#664]: https://github.com/notify-rs/notify/pull/664
[#680]: https://github.com/notify-rs/notify/pull/680
[#701]: https://github.com/notify-rs/notify/pull/701

## debouncer-full 0.5.0 (2025-01-10)

- CHANGE: update notify to version 8.0.0
- CHANGE: pass `web-time` feature to notify-types

## debouncer-full 0.4.0 (2024-10-25)

- CHANGE: update notify to version 7.0.0
- CHANGE: manage root folder paths for the file ID cache automatically [#557] **breaking**

  ```rust
  debouncer.watcher().watch(path, RecursiveMode::Recursive)?;
  debouncer.cache().add_root(path, RecursiveMode::Recursive);
  ```

  becomes:

  ```rust
  debouncer.watch(path, RecursiveMode::Recursive)?;
  ```

- CHANGE: add `RecommendedCache`, which automatically enables the file ID cache on Windows and MacOS
  and disables it on Linux, where it is not needed [#557]

[#557]: https://github.com/notify-rs/notify/pull/557

## debouncer-full 0.3.2 (2024-09-29)

- FIX: ordering of debounced events could lead to a panic with Rust 1.81.0 and above [#636]

[#636]: https://github.com/notify-rs/notify/issues/636

## debouncer-full 0.3.1 (2023-08-21)

- CHANGE: remove serde binary experiment opt-out after it got removed [#530]

[#530]: https://github.com/notify-rs/notify/pull/530

## debouncer-full 0.3.0 (2023-08-18)

- CHANGE: opt-out of the serde binary experiment by restricting it to < 1.0.172 [#528]
- CHANGE: license changed to dual-license of MIT OR Apache-2.0 [#520]
- CHANGE: upgrade to file-id 0.2.0 for high resolution file IDs [#494]
- FEATURE: derive debug for the debouncer struct [#510]

[#494]: https://github.com/notify-rs/notify/pull/494
[#510]: https://github.com/notify-rs/notify/pull/510
[#520]: https://github.com/notify-rs/notify/pull/520
[#528]: https://github.com/notify-rs/notify/pull/528

## debouncer-full 0.2.0 (2023-06-16)

- CHANGE: emit events as `DebouncedEvent`s, each containing the original notify event and the time at which it occurred [#488]

[#488]: https://github.com/notify-rs/notify/pull/488

## debouncer-full 0.1.0 (2023-05-17)

Newly introduced alternative debouncer with more features. [#480]

- FEATURE: only emit a single `rename` event if the rename `From` and `To` events can be matched
- FEATURE: merge multiple `rename` events
- FEATURE: keep track of the file system IDs all files and stitches rename events together (FSevents, Windows)
- FEATURE: emit only one `remove` event when deleting a directory (inotify)
- FEATURE: don't emit duplicate create events
- FEATURE: don't emit `Modify` events after a `Create` event

[#480]: https://github.com/notify-rs/notify/pull/480
