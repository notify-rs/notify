# Changelog
## debouncer-full 0.7.1 (unreleased)

- FEATURE: impl `EventHandler` for `futures::channel::mpsc::UnboundedSender` and `tokio::sync::mpsc::UnboundedSender` behind the `futures` and `tokio` feature flags [#767]

[#767]: https://github.com/notify-rs/notify/pull/767

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
