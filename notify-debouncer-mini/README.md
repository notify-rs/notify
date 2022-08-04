# Notify debouncer

[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]

Tiny debouncer for notify. Filters incoming events and emits only one event per timeframe per file.

## Features

- `crossbeam` enabled by default, for crossbeam channel support.  
This may create problems used in tokio environments. See [#380](https://github.com/notify-rs/notify/issues/380).  
Use someting like `notify-debouncer-mini = { version = "*", default-features = false }` to disable it.
- `serde` for serde support of event types, off by default

[docs]: https://docs.rs/notify/0.1/notify-debouncer-mini/