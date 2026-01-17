# Notify debouncer

[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]

Tiny debouncer for [notify]. Filters incoming events and emits only one event per timeframe per file.

## Features

- `crossbeam-channel` passed down to notify, off by default

- `flume` passed down to notify, off by default

- `serde` for serde support of event types, off by default

- `serialization-compat-6` passed down to notify, off by default

## Minimum Supported Rust Version (MSRV) Policy

We follow these MSRV rules:

- The current MSRV is **1.85**.
- MSRV bumps do NOT require a major release and may happen in minor releases.
- The MSRV may be updated when needed, but support for the current stable Rust release and the previous two stable releases (N, N-1, N-2) is always guaranteed.
  - For example, if the current stable version is 1.85, we guarantee support for 1.85, 1.84, and 1.83, so the minimum supported Rust version will be **at most** 1.83.
- MSRV is bumped only when required by dependencies or when adopting new stable Rust features.
- Every MSRV bump is documented in the release notes when it happens.

[docs]: https://docs.rs/notify-debouncer-mini
[notify]: https://crates.io/crates/notify
