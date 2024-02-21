# Notify debouncer

[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]

Tiny debouncer for [notify]. Filters incoming events and emits only one event per timeframe per file.

## Features

- `crossbeam` enabled by default, for crossbeam channel support.

  This may create problems used in tokio environments. See [#380](https://github.com/notify-rs/notify/issues/380).
  Use something like the following to disable it.

  ```toml
  notify-debouncer-mini = { version = "*", default-features = false }
  ```

  This also passes through to notify as `crossbeam-channel` feature.

  On MacOS, when disabling default features, enable either the `macos_fsevent` feature
  or, on latest MacOS, the `macos_kqueue` feature to be passed through to notify.

  ```toml
  # Using FSEvents
  notify-debouncer-mini = { version = "*", default-features = false, features = ["macos_fsevent"] }

  # Using Kernel Queues
  notify-debouncer-mini = { version = "*", default-features = false, features = ["macos_kqueue"] }
  ```
- `serde` for serde support of event types, off by default

- `serialization-compat-6` passed down to notify, off by default

[docs]: https://docs.rs/notify-debouncer-mini
[notify]: https://crates.io/crates/notify
