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

- `serialization-compat-6` passed down to notify, off by default

[docs]: https://docs.rs/notify-debouncer-full
[notify]: https://crates.io/crates/notify
