# Changelog

## debouncer-mini 0.7.1 (unreleased)

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
