# Notify debouncer

[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]

Tiny debouncer for [notify]. Filters incoming events and emits only one event per timeframe per file.

Successful notifications are either `DebouncedEventBatch::Events`, containing debounced path
changes, or `DebouncedEventBatch::Rescan`, indicating that cached filesystem state must be
refreshed for all watched paths. Rescan notifications are delivered without waiting for the
debounce timeout and must be handled before filtering individual events by path or kind.

Version 0.8 uses notify v9 and changes `DebounceEventResult` from a vector of events to this enum.
See the [migration guide](../docs/UPGRADING_V8_TO_V9.md#5-mini-debouncer-notifications) for an example.

## Features

- `crossbeam-channel` passed down to notify, off by default

- `flume` passed down to notify, off by default

- `serde` for serde support of event types, off by default

- `serialization-compat-6` passed down to notify, off by default

## Minimum Supported Rust Version (MSRV) Policy

We follow these MSRV rules:

- The current MSRV is **1.88**.
- MSRV bumps do NOT require a major release and may happen in minor releases.
- The MSRV may be updated when needed, but support for the current stable Rust release and the previous two stable releases (N, N-1, N-2) is always guaranteed.
  - For example, if the current stable version is 1.85, we guarantee support for 1.85, 1.84, and 1.83, so the minimum supported Rust version will be **at most** 1.83.
- MSRV is bumped only when needed, e.g.:
  - required by dependencies
  - any serious bug is found (including security-related)
  - adopting new stable Rust features
- Every MSRV bump is documented in the release notes when it happens.

[docs]: https://docs.rs/notify-debouncer-mini
[notify]: https://crates.io/crates/notify
