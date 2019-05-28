# Notify

[![» Crate](https://flat.badgen.net/crates/v/notify)][crate]
[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]
[![» CI](https://flat.badgen.net/travis/passcod/notify/main)][build]
[![» Downloads](https://flat.badgen.net/crates/d/notify)][crate]
[![» Conduct](https://flat.badgen.net/badge/contributor/covenant/5e0d73)][coc]
[![» Public Domain](https://flat.badgen.net/badge/license/CC0-1.0/purple)][cc0]

_Cross-platform filesystem notification library for Rust._

**This is the readme for the upcoming 5.0 release! Look at [the 4.0 one] instead.**

[the 4.0 one]: https://github.com/passcod/notify/tree/v4.0.10#notify

(Looking for desktop notifications instead? Have a look at [notify-rust] or
[alert-after]!)

- **[Guides and in-depth docs][wiki]**
- [API Documentation][docs]
- [Crate page][crate]
- [Changelog][changelog]
- **[Upgrading from 4.0](https://github.com/passcod/notify/wiki/Upgrading-from-4.0-to-5.0)**
- Earliest supported Rust version: **1.32.0**

As used by: [alacritty], [cargo watch], [cobalt], [docket], [handlebars-iron],
[mdBook], [pax], [rdiff], [rust-analyzer], [timetrack], [watchexec],
[xi-editor], and others. (Want to be added to this list? Open a pull request!)

## Installation

```toml
[dependencies]
notify = "5.0.0"
```

## Usage

```rust
use notify::{RecommendedWatcher, RecursiveMode, Result, watcher};
use std::time::Duration;

fn main() -> Result<()> {
    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let mut watcher = watcher(Duration::from_secs(2))?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch("/home/test/notify", RecursiveMode::Recursive)?;

    // This is a simple loop, but you may want to use more complex logic here,
    // for example to handle I/O.
    for event in &watcher {
        match event {
            Ok(event) => println!("changed: {:?}", event.path),
            Err(err) => println!("watch error: {:?}", err),
        };
    }

    Ok(())
}
```

### With a channel

To get a channel for advanced or flexible cases, use:

```rust
let rx = watcher.channel();

loop {
    match rx.recv() {
        // ...
    }
}
```

To pass in a channel manually:

```rust
let (tx, rx) = crossbeam_channel::unbounded();
let mut watcher: RecommendedWatcher = Watcher::with_channel(tx, Duration::from_secs(2))?;

for event in rx.iter() {
    // ...
}
```

### With precise events

By default, Notify issues generic events that carry little additional
information beyond what path was affected. On some platforms, more is
available; stay aware though that how exactly that manifests varies. To enable
precise events, use:

```rust
use notify::Config;
watcher.configure(Config::PreciseEvents(true));
```

### With notice events

Sometimes you want to respond to some events straight away, but not give up the
advantages of debouncing. Notice events appear once immediately when the occur
during a debouncing period, and then a second time as usual at the end of the
debouncing period:

```rust
use notify::Config;
watcher.configure(Config::NoticeEvents(true));
```

### With ongoing events

Sometimes frequent writes may be missed or not noticed often enough. Ongoing
write events can be enabled to emit more events even while debouncing:

```rust
use notify::Config;
watcher.configure(Config::OngoingEvents(Some(Duration::from_millis(500))));
```

### Without debouncing

To receive events as they are emitted, without debouncing at all:

```rust
let mut watcher = immediate_watcher()?;
```

With a channel:

```rust
let (tx, rx) = unbounded();
let mut watcher: RecommendedWatcher = Watcher::immediate_with_channel(tx)?;
```

### Serde

Events can be serialisable via [serde]. To enable the feature:

```toml
notify = { version = "5.0.0", features = ["serde"] }
```

## Platforms

- Linux / Android: inotify
- macOS: FSEvents
- Windows: ReadDirectoryChangesW
- All platforms: polling

### FSEvents

Due to the inner security model of FSEvents (see [FileSystemEventSecurity]),
some event cannot be observed easily when trying to follow files that do not
belong to you. In this case, reverting to the pollwatcher can fix the issue,
with a slight performance cost.

## Next generation

While this current version continues to be developed and maintained, a next
generation design of the library lives in the
[`next`](https://github.com/passcod/notify/tree/next) branch. There is no solid
ETA, beyond that most of it will not be released before `async`/`await` is
stabilised in Rust. For an overview and background, see [this draft
announce](https://github.com/passcod/notify/wiki/Presentation).

Instead of one large release, though, it is much more likely that smaller
components of the design, once they have gone through revising and maturing in
the `next` branch, will be incorporated in the `main` branch. The first large
piece, a new event classification system, landed in 5.0.

Do be aware of the licensing difference. Notify is so far under [CC0][cc0]. The
`next` branch is instead under the [Artistic License 2.0][artistic]. Pieces of
the `next` branch brought to `main` are dual-licensed under CC0, but the eventual
plan is to be entirely Artistic License 2.0 code. The formal license change
_will_ incur a major version bump.

## Origins

Inspired by Go's [fsnotify] and Node.js's [Chokidar], born out of need for
[cargo watch], and general frustration at the non-existence of C/Rust
cross-platform notify libraries.

Written by [Félix Saparelli] and awesome [contributors].

[Chokidar]: https://github.com/paulmillr/chokidar
[FileSystemEventSecurity]: https://developer.apple.com/library/mac/documentation/Darwin/Conceptual/FSEvents_ProgGuide/FileSystemEventSecurity/FileSystemEventSecurity.html
[Félix Saparelli]: https://passcod.name
[alacritty]: https://github.com/jwilm/alacritty
[alert-after]: https://github.com/frewsxcv/alert-after
[artistic]: ./LICENSE.ARTISTIC
[build]: https://travis-ci.org/passcod/notify
[cargo watch]: https://github.com/passcod/cargo-watch
[cc0]: ./LICENSE
[changelog]: ./CHANGELOG.md
[cobalt]: https://github.com/cobalt-org/cobalt.rs
[coc]: http://contributor-covenant.org/version/1/4/
[contributors]: https://github.com/passcod/notify/graphs/contributors
[crate]: https://crates.io/crates/notify
[docket]: https://iwillspeak.github.io/docket/
[docs]: https://docs.rs/notify
[fsnotify]: https://github.com/go-fsnotify/fsnotify
[handlebars-iron]: https://github.com/sunng87/handlebars-iron
[hotwatch]: https://github.com/francesca64/hotwatch
[mdBook]: https://github.com/rust-lang-nursery/mdBook
[notify-rust]: https://github.com/hoodie/notify-rust
[pax]: https://pax.js.org/
[rdiff]: https://github.com/dyule/rdiff
[rust-analyzer]: https://github.com/rust-analyzer/rust-analyzer
[serde]: https://serde.rs/
[timetrack]: https://github.com/joshmcguigan/timetrack
[watchexec]: https://github.com/mattgreen/watchexec
[wiki]: https://github.com/passcod/notify/wiki
[xi-editor]: https://xi-editor.io/
