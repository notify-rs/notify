# Notify

_Cross-platform filesystem notification library for Rust._

## Install

```toml
[dependencies.notify]
git = "https://github.com/passcod/rsnotify.git"
```

Will be added to the [crates.io](https://crates.io) registry as soon as the
all the dependencies are published there as well.

## Usage

```rust
extern crate notify;

use notify::{RecommendedWatcher, Error, Watcher};

fn main() {
  // Create a channel to receive the events.
  let (tx, rx) = channel();

  // Automatically select the best implementation for your platform.
  // You can also access each implementation directly e.g. PollWatcher.
  let mut w: Result<RecommendedWatcher, Error> = Watcher::new(tx);

  match w {
    Ok(mut watcher) => {
      // Add a path to be watched. All files and directories at that path and
      // below will be monitored for changes.
      watcher.watch(&Path::new("/home/test/notify"));

      // You'll probably want to do that in a loop. The type to match for is
      // notify::Event, look at src/lib.rs for details.
      match rx.recv() {
        _ => println!("Recv.")
      }
    },
    Err(e) => println!("Error")
  }
}
```

## Platforms

- Linux / Android: inotify
- All platforms: polling (only `op::WRITE`)

### Todo

- Windows: ReadDirectoryChangesW
- OS X: FSEvents
- BSD / OS X / iOS: kqueue
- Solaris 11: FEN

## Known Bugs

- inotify backend panics when dropped
- polling backend only handles `op::WRITE`s
- see `TODO` comments in the code for more

## Origins

Inspired by Go's [fsnotify](https://github.com/go-fsnotify/fsnotify), born out
of need for [cargo watch](https://github.com/passcod/cargo-watch), and general
frustration at the non-existence of C/Rust cross-platform notify libraries.

Written from scratch by [Félix Saparelli](https://passcod.name), and released
in the Public Domain.
