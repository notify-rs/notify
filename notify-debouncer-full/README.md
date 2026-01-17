# Notify Debouncer Full

[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]

A debouncer for [notify] that is optimized for ease of use.

* Only emits a single `Rename` event if the rename `From` and `To` events can be matched
* Merges multiple `Rename` events
* Takes `Rename` events into account and updates paths for events that occurred before the rename event, but which haven't been emitted, yet
* Optionally keeps track of the file system IDs all files and stitches rename events together (FSevents, Windows)
* Emits only one `Remove` event when deleting a directory (inotify)
* Doesn't emit duplicate create events
* Doesn't emit `Modify` events after a `Create` event

## Features

- `crossbeam-channel` passed down to notify, off by default

- `flume` passed down to notify, off by default

- `serialization-compat-6` passed down to notify, off by default

## Minimum Supported Rust Version (MSRV) Policy

We follow these MSRV rules:

- The current MSRV is **1.85**.
- MSRV bumps do NOT require a major release and may happen in minor releases.
- The MSRV may be updated when needed, but support for the current stable Rust release and the previous two stable releases (N, N-1, N-2) is always guaranteed.
  - For example, if the current stable version is 1.85, we guarantee support for 1.85, 1.84, and 1.83, so the minimum supported Rust version will be **at most** 1.83.
- MSRV is bumped only when required by dependencies or when adopting new stable Rust features.
- Every MSRV bump is documented in the release notes when it happens.

[docs]: https://docs.rs/notify-debouncer-full
[notify]: https://crates.io/crates/notify
