# Notify

_Cross-platform filesystem notification library for Rust._

## Install

```toml
[dependencies]
notify = "^1.0"
```

## Usage

```rust
extern crate notify;

use notify::{RecommendedWatcher, Error, Watcher};

fn main() {
  let (tx, rx) = channel();
  let mut w: Result<RecommendedWatcher, Error> = Watcher::new(tx);
  match w {
    Ok(mut watcher) => {
      watcher.watch(&Path::new("/home/test/notify"));
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
