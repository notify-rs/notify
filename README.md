# Notify

[![Crate version](https://img.shields.io/crates/v/notify.svg?style=flat-square)](https://crates.io/crates/notify)
[![Crate license](https://img.shields.io/crates/l/notify.svg?style=flat-square)](https://creativecommons.org/publicdomain/zero/1.0/)
![Crate download count](https://img.shields.io/crates/d/notify.svg?style=flat-square)

[![Appveyor](https://img.shields.io/appveyor/ci/passcod/rsnotify.svg?style=flat-square)](https://ci.appveyor.com/project/passcod/rsnotify) <sup>(Windows)</sup>
[![Travis](https://img.shields.io/travis/passcod/rsnotify.svg?style=flat-square)](https://travis-ci.org/passcod/rsnotify) <sup>(Linux and OS X)</sup>

[![Code of Conduct](https://img.shields.io/badge/contributor-covenant-123456.svg?style=flat-square)](http://contributor-covenant.org/version/1/3/0/)
[![Documentation](https://img.shields.io/badge/documentation-docs.rs-df3600.svg?style=flat-square)](https://docs.rs/notify)


_Cross-platform filesystem notification library for Rust._

## Installation

```toml
[dependencies]
notify = "2.6.3"
```

## Usage

```rust
extern crate notify;
use notify::{RecommendedWatcher, Watcher};
use std::sync::mpsc::channel;

fn watch() -> notify::Result<()> {
  // Create a channel to receive the events.
  let (tx, rx) = channel();

  // Automatically select the best implementation for your platform.
  // You can also access each implementation directly e.g. INotifyWatcher.
  let mut watcher: RecommendedWatcher = try!(Watcher::new_raw(tx));

  // Add a path to be watched. All files and directories at that path and
  // below will be monitored for changes.
  try!(watcher.watch("/home/test/notify"));

  // This is a simple loop, but you may want to use more complex logic here,
  // for example to handle I/O.
  loop {
      match rx.recv() {
        Ok(notify::RawEvent{ path: Some(path),op:Ok(op) }) => {
            println!("{:?} {:?}", op, path);
        },
        Err(e) => println!("watch error {}", e),
        _ => ()
      }
  }
}

fn main() {
  if let Err(err) = watch() {
    println!("Error! {:?}", err)
  }
}
```

## Migration

### From v2.x to v3.x

* `notify` now provides two APIs, a _raw_ and a _debounced_ API. In order to keep the old behavior, use the _raw_ API.
Replace every occurrence of `Watcher::new` with `Watcher::new_raw` and `Event` with `RawEvent`. Or see the docs for how to use the _debounced_ API.
* The watch(..) function used to watch a file or a directory now takes an additional argument.
In order to use that argument you first need to import `RecursiveMode` via the `use` keyword.
To keep the old behavior, use `RecursiveMode::Recursive`, for more information see the docs.
* The inotify back-end used to add watches recursively to a directory but it wouldn't remove them recursively.
From v3.0.0 on inotify removes watches recursively if they were added recursively.
* The inotify back-end didn't use to watch newly created directories.
From v3.0.0 on inotify watches newly created directories if their parent directories were added recursively.

## Platforms

- Linux / Android: inotify
- OS X: FSEvents
- Windows: ReadDirectoryChangesW
- All platforms: polling

## Limitations

### FSEvents

Due to the inner security model of FSEvents (see [FileSystemEventSecurity](https://developer.apple.com/library/mac/documentation/Darwin/Conceptual/FSEvents_ProgGuide/FileSystemEventSecurity/FileSystemEventSecurity.html)), some event cannot be observed easily when trying to follow files that do not belong to you. In this case, reverting to the pollwatcher can fix the issue, with a slight performance cost.

## Todo

- BSD / OS X / iOS: kqueue
- Solaris 11: FEN

Pull requests and bug reports happily accepted!

## Origins

Inspired by Go's [fsnotify](https://github.com/go-fsnotify/fsnotify), born out
of need for [cargo watch](https://github.com/passcod/cargo-watch), and general
frustration at the non-existence of C/Rust cross-platform notify libraries.

Written by [Félix Saparelli](https://passcod.name) and awesome
[contributors](https://github.com/passcod/rsnotify/graphs/contributors),
and released in the Public Domain using the Creative Commons Zero Declaration.
