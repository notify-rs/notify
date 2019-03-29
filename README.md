# Notify

[![» Crate](https://flat.badgen.net/crates/v/notify)][crate]
[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]
[![» CI](https://flat.badgen.net/travis/passcod/notify/main)][build]
[![» Downloads](https://flat.badgen.net/crates/d/notify)][crate]
[![» Conduct](https://flat.badgen.net/badge/contributor/covenant/5e0d73)][coc]
[![» Public Domain](https://flat.badgen.net/badge/license/CC0-1.0/purple)][cc0]

_Cross-platform filesystem notification library for Rust._

(Looking for desktop notifications instead? Have a look at [notify-rust] or
[alert-after]!)

- [API Documentation][docs]
- [Crate page][crate]
- [Changelog][changelog]
- Earliest supported Rust version: **1.26.1**

As used by: [alacritty], [cargo watch], [cobalt], [docket], [handlebars-iron],
[mdBook], [pax], [rdiff], [timetrack], [watchexec], [xi-editor], and others.
(Want to be added to this list? Open a pull request!)

## Installation

```toml
[dependencies]
notify = "4.0.0"
```

## Usage

```rust
extern crate notify;

use notify::{RecommendedWatcher, Watcher, RecursiveMode};
use std::sync::mpsc::channel;
use std::time::Duration;

fn watch() -> notify::Result<()> {
    // Create a channel to receive the events.
    let (tx, rx) = channel();

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let mut watcher: RecommendedWatcher = try!(Watcher::new(tx, Duration::from_secs(2)));

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    try!(watcher.watch("/home/test/notify", RecursiveMode::Recursive));

    // This is a simple loop, but you may want to use more complex logic here,
    // for example to handle I/O.
    loop {
        match rx.recv() {
            Ok(event) => println!("{:?}", event),
            Err(e) => println!("watch error: {:?}", e),
        }
    }
}

fn main() {
    if let Err(e) = watch() {
        println!("error: {:?}", e)
    }
}
```

Looking overly verbose? Too much boilerplate? Have a look at [hotwatch], a friendly wrapper:

```rust
// Taken from the hotwatch readme
use hotwatch::{Hotwatch, Event};

let mut hotwatch = Hotwatch::new().expect("Hotwatch failed to initialize.");
hotwatch.watch("war.png", |event: Event| {
    if let Event::Write(path) = event {
        println!("War has changed.");
    }
}).expect("Failed to watch file!");
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

As usual, pull requests for fixes and features are welcome!

Do be aware of the licensing difference. Notify is so far under [CC0][cc0]. The
`next` branch is instead under the [Artistic License 2.0][artistic]. Pieces of
the `next` branch brought to `main` are re-licensed under CC0, but the eventual
plan is to be entirely Artistic License 2.0 code. The formal license change
_will_ incur a major version bump.

## Origins

Inspired by Go's [fsnotify] and Node.js's [Chokidar], born out of need for
[cargo watch], and general frustration at the non-existence of C/Rust
cross-platform notify libraries.

Written by [Félix Saparelli] and awesome [contributors], and released in the
Public Domain using the [Creative Commons Zero Declaration][cc0].

[Chokidar]: https://github.com/paulmillr/chokidar
[FileSystemEventSecurity]: https://developer.apple.com/library/mac/documentation/Darwin/Conceptual/FSEvents_ProgGuide/FileSystemEventSecurity/FileSystemEventSecurity.html
[Félix Saparelli]: https://passcod.name
[alert-after]: https://github.com/frewsxcv/alert-after
[alacritty]: https://github.com/jwilm/alacritty
[artistic]: https://github.com/passcod/notify/blob/next/LICENSE
[build]: https://travis-ci.org/passcod/notify
[cargo watch]: https://github.com/passcod/cargo-watch
[cc0]: https://creativecommons.org/publicdomain/zero/1.0/
[changelog]: ./CHANGELOG.md
[cobalt]: https://github.com/cobalt-org/cobalt.rs
[coc]: http://contributor-covenant.org/version/1/4/
[contributors]: https://github.com/passcod/notify/graphs/contributors
[crate]: https://crates.io/crates/notify
[docs-debounce]: https://docs.rs/notify/#default-debounced-api
[docs-raw]: https://docs.rs/notify/#raw-api
[docs-recursivemode]: https://docs.rs/notify/*/notify/enum.RecursiveMode.html
[docs]: https://docs.rs/notify
[docket]: https://iwillspeak.github.io/docket/
[fsnotify]: https://github.com/go-fsnotify/fsnotify
[handlebars-iron]: https://github.com/sunng87/handlebars-iron
[hotwatch]: https://github.com/francesca64/hotwatch
[mdBook]: https://github.com/rust-lang-nursery/mdBook
[notify-rust]: https://github.com/hoodie/notify-rust
[pax]: https://pax.js.org/
[rdiff]: https://github.com/dyule/rdiff
[timetrack]: https://github.com/joshmcguigan/timetrack
[watchexec]: https://github.com/mattgreen/watchexec
[xi-editor]: https://xi-editor.io/
