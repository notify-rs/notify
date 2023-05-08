# Notify Debouncer Refined

[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]

A debouncer for [notify] that is optimized for ease of use.

* Only emits a single `Rename` event if the rename `From` and `To` events can be matched
* Merges multiple `Rename` events
* Optionally keeps track of the file system IDs all files and stiches rename events together (FSevents, Windows)
* Emits only one `Remove` event when deleting a directory (inotify)
* Doesn't emit duplicate create events
* Doesn't emit `Modify` events after a `Create` event

## Features

- `crossbeam` enabled by default, for crossbeam channel support.

  This may create problems used in tokio environments. See [#380](https://github.com/notify-rs/notify/issues/380).  
  Use someting like the following to disable it.
  
  ```toml
  notify-debouncer-refined = { version = "*", default-features = false }
  ```
  
  This also passes through to notify as `crossbeam-channel` feature.

- `serde` for serde support of event types, off by default

[docs]: https://docs.rs/notify-debouncer-refined
[notify]: https://crates.io/crates/notify
