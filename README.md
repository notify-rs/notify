# Notify

[![Crate version](https://img.shields.io/crates/v/notify.svg?style=flat-square)][crate]
[![Crate license](https://img.shields.io/crates/l/notify.svg?style=flat-square)][cc0]
[![Crate download count](https://img.shields.io/crates/d/notify.svg?style=flat-square)][crate]

[![Appveyor](https://img.shields.io/appveyor/ci/passcod/rsnotify.svg?style=flat-square)][build-windows] <sup>(Windows)</sup>
[![Travis](https://img.shields.io/travis/passcod/notify.svg?style=flat-square)][build-unix] <sup>(Linux and OS X)</sup>

[![Code of Conduct](https://img.shields.io/badge/contributor-covenant-123456.svg?style=flat-square)][coc]
[![Documentation](https://img.shields.io/badge/documentation-docs.rs-df3600.svg?style=flat-square)][docs]


_Cross-platform filesystem notification library for Rust._


As used by: [cargo watch], [cobalt], [handlebars-iron], [rdiff], and
[watchexec]. (Want to be added to this list? Open a pull request!)

## Installation

```toml
[dependencies]
notify = "3.0.1"
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

## Version 2.x

The documentation for the previous major version is [available on
docs.rs][docs-v2]. While version 2.x will no longer be maintained and we
encourage all library authors to switch to version 3 (a short guide is provided
below), it is still a dependency of many packages. Here is a list of changes
you may need to take note of:

- Notify 2.x by default provided the events immediately as reported from the
  backend API. Notify 3.x by default [debounces the events][docs-debounce] — if
  the backend reports two similar events in close succession, Notify will only
  report one. The old behaviour may be obtained through the
  `Watcher::new_raw()` function and `RawEvent` type, see [the
  documentation][docs-raw].

- Notify 2.x always tried to watch paths recursively in the case of
  directories. Notify 3.x gives you the choice of what mode you'd like to use
  per-watch, using the [`RecursiveMode`][docs-recursivemode] enum. The
  `watch(...)` function thus takes the mode as a second argument.

- Notify 2.x had two behaviour bugs with the **inotify** backend, that are
  corrected in Notify 3.x. Nonetheless, these are breaking changes:

  * **inotify** did not _remove_ watches recursively; and
  * **inotify** did not watch _newly created folders_.

To upgrade to Notify 3.x with minimal behaviour change:

- Replace `Watcher::new` with `Watcher::new_raw`.
- Replace `Event` with `EventRaw`.
- Import `notify::RecursiveMode` and add `RecursiveMode::Recursive` as second
  argument to the `watch()` function.

## Platforms

- Linux / Android: inotify
- OS X: FSEvents
- Windows: ReadDirectoryChangesW
- All platforms: polling

### FSEvents

Due to the inner security model of FSEvents (see [FileSystemEventSecurity]),
some event cannot be observed easily when trying to follow files that do not
belong to you. In this case, reverting to the pollwatcher can fix the issue,
with a slight performance cost.

## Todo

- BSD / OS X / iOS: kqueue
- Solaris 11: FEN

Pull requests and bug reports happily accepted!

## Origins

Inspired by Go's [fsnotify] and Node.js's [Chokidar], born out of need for
[cargo watch], and general frustration at the non-existence of C/Rust
cross-platform notify libraries.

Written by [Félix Saparelli] and awesome [contributors], and released in the
Public Domain using the [Creative Commons Zero Declaration][cc0].

[Chokidar]: https://github.com/paulmillr/chokidar
[FileSystemEventSecurity]: https://developer.apple.com/library/mac/documentation/Darwin/Conceptual/FSEvents_ProgGuide/FileSystemEventSecurity/FileSystemEventSecurity.html
[Félix Saparelli]: https://passcod.name
[build-unix]: https://travis-ci.org/passcod/notify
[build-windows]: https://ci.appveyor.com/project/passcod/rsnotify
[cargo watch]: https://github.com/passcod/cargo-watch
[cc0]: https://creativecommons.org/publicdomain/zero/1.0/
[cobalt]: https://github.com/cobalt-org/cobalt.rs
[coc]: http://contributor-covenant.org/version/1/4/
[contributors]: https://github.com/passcod/notify/graphs/contributors
[crate]: https://crates.io/crates/notify
[docs-debounce]: https://docs.rs/notify/#default-debounced-api
[docs-raw]: https://docs.rs/notify/#raw-api
[docs-recursivemode]: https://docs.rs/notify/enum.RecursiveMode.html
[docs-v2]: https://docs.rs/notify/2
[docs]: https://docs.rs/notify
[fsnotify]: https://github.com/go-fsnotify/fsnotify
[handlebars-iron]: https://github.com/sunng87/handlebars-iron
[rdiff]: https://github.com/dyule/rdiff
[watchexec]: https://github.com/mattgreen/watchexec
