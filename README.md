# Notify

[![Crate version](https://img.shields.io/crates/v/notify.svg?style=flat-square)][crate]
[![Crate license](https://img.shields.io/crates/l/notify.svg?style=flat-square)][cc0]
[![Crate download count](https://img.shields.io/crates/d/notify.svg?style=flat-square)][crate]

[![Appveyor](https://img.shields.io/appveyor/ci/passcod/rsnotify.svg?style=flat-square)][build-windows] <sup>(Windows)</sup>
[![Travis](https://img.shields.io/travis/passcod/notify.svg?style=flat-square)][build-unix] <sup>(Linux and macOS)</sup>

[![Code of Conduct](https://img.shields.io/badge/contributor-covenant-123456.svg?style=flat-square)][coc]
[![Documentation](https://img.shields.io/badge/documentation-docs.rs-df3600.svg?style=flat-square)][docs]


_Cross-platform filesystem notification library for Rust._


As used by: [cargo watch], [cobalt], [handlebars-iron], [rdiff], [docket],
[watchexec], and [timetrack]. (Want to be added to this list? Open a pull request!)

Version Next status and progress: [branch `next`](https://github.com/passcod/notify/tree/next#status).

As a clarification: **version 4 is not "frozen"!** I'm just not actively spending time on it. (Originally I thought that Version Next or "5" would take less time to get out, so I prepared for not doing anything with Version 4 anymore, but it has now been clear for a while that the finish line for Version Next is quite far away still.) I do accept pull requests for fixes _and features_, and would even consider breaking changes with enough justification. Do contribute, please!

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

## Todo

Further development happens on the `next` branch for version 5. Development for
version 4 (this version) is frozen: there will be no new features, only bug
fixes and documentation updates.

## Origins

Inspired by Go's [fsnotify] and Node.js's [Chokidar], born out of need for
[cargo watch], and general frustration at the non-existence of C/Rust
cross-platform notify libraries.

Written by [Félix Saparelli] and awesome [contributors], and released in the
Public Domain using the [Creative Commons Zero Declaration][cc0].

Note that licensing is changed from version 5 to **Artistic 2.0**.

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
[docs-recursivemode]: https://docs.rs/notify/*/notify/enum.RecursiveMode.html
[docs-v2]: https://docs.rs/notify/2
[docs]: https://docs.rs/notify
[docket]: https://iwillspeak.github.io/docket/
[fsnotify]: https://github.com/go-fsnotify/fsnotify
[handlebars-iron]: https://github.com/sunng87/handlebars-iron
[rdiff]: https://github.com/dyule/rdiff
[watchexec]: https://github.com/mattgreen/watchexec
[timetrack]: https://github.com/joshmcguigan/timetrack
