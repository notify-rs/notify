# Changelog

## unreleased

## debouncer-mini 0.8.0 (2026-11-21)

This release uses notify 9.0.0 and requires Rust 1.91. For an updated event handler, see the [0.7 to 0.8 migration guide](../docs/UPGRADING_V8_TO_V9.md#5-mini-debouncer-notifications).

- `DebounceEventResult` delivers path changes in `DebouncedEventBatch::Events`. A `DebouncedEventBatch::Rescan` is delivered immediately when events may have been missed. Refresh all watched paths on receipt.
- The default event filter excludes access events.

## debouncer-mini 0.8.0-rc.2 (2026-11-15)

- CHANGE: upgrade `notify` to 9.0.0-rc.9 and `notify-types` to 2.3.1
- CHANGE: `DebouncedEventBatch` is now `#[non_exhaustive]`, requiring a wildcard arm when matching successful notifications. See the [migration guide](../docs/UPGRADING_V8_TO_V9.md#5-mini-debouncer-notifications) [#1106] **breaking**
- CHANGE: `Config::default()` uses `EventKindMask::CORE`, excluding access events. See the [migration guide](../docs/UPGRADING_V8_TO_V9.md#event-kind-filtering-was-added) [#1103]
- DEPS: move `tempfile` to dev-dependencies [#1099]

[#1099]: https://github.com/notify-rs/notify/pull/1099
[#1103]: https://github.com/notify-rs/notify/pull/1103
[#1106]: https://github.com/notify-rs/notify/pull/1106

## debouncer-mini 0.8.0-rc.1 (2026-11-09)

- CHANGE: update notify to version 9.0.0-rc.8 **breaking**
- CHANGE: `DebounceEventResult` now returns `DebouncedEventBatch::Events(Vec<DebouncedEvent>)` or `DebouncedEventBatch::Rescan` on success **breaking**
- FIX: forward rescan notifications immediately, with or without paths, so callers can refresh state for all watched paths
- CHANGE: raise MSRV to 1.91
- PERF: reuse the pending-event map with `HashMap::extract_if` instead of rebuilding it on every debounce tick
- CHANGE: add `#[must_use]` annotations to debouncer config builder APIs
- FEATURE: impl `EventHandler` for `futures::channel::mpsc::UnboundedSender` and `tokio::sync::mpsc::UnboundedSender` behind the `futures` and `tokio` feature flags [#767]

[#767]: https://github.com/notify-rs/notify/pull/767

## debouncer-mini 0.7.0 (2025-08-03)
- FEATURE: added support for the [`flume`](https://docs.rs/flume) crate [#680]

[#680]: https://github.com/notify-rs/notify/pull/680

## debouncer-mini 0.6.0 (2025-01-10)

- CHANGE: update notify to version 8.0.0

## debouncer-mini 0.5.0 (2024-10-25)

- CHANGE: update notify to version 7.0.0

## debouncer-mini 0.4.1 (2023-08-21)

- CHANGE: remove serde binary experiment opt-out after it got removed [#530]

[#530]: https://github.com/notify-rs/notify/pull/530

## debouncer-mini 0.4.0 (2023-08-18)

- CHANGE: opt-out of the serde binary experiment by restricting it to < 1.0.172 [#528]
- CHANGE: license changed to dual-license of MIT OR Apache-2.0 [#520]
- CHANGE: replace active polling with passive loop, removing empty ticks [#467]
- FEATURE: derive debug for the debouncer struct [#510]

[#467]: https://github.com/notify-rs/notify/pull/467
[#510]: https://github.com/notify-rs/notify/pull/510
[#520]: https://github.com/notify-rs/notify/pull/520
[#528]: https://github.com/notify-rs/notify/pull/528

## debouncer-mini 0.3.0 (2023-05-17)

- CHANGE: upgrade to notify 6.0.0, pushing MSRV to 1.60 [#480]

[#480]: https://github.com/notify-rs/notify/pull/480

## debouncer-mini 0.2.1 (2022-09-05)

- DOCS: correctly document the `crossbeam` feature [#440]

[#440]: https://github.com/notify-rs/notify/pull/440

## debouncer-mini 0.2.0 (2022-08-30)

Upgrade notify dependency to 5.0.0
