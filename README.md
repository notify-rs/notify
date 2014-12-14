# Notify

_Cross-platform filesystem notification library for Rust._

## Install

```toml
[dependencies.notify]
git = "https://github.com/passcod/rsnotify.git"
```

## Usage

```rust
extern crate notify;

// Create a channel to receive the events
let (tx, rx) = channel();

// Select the recommended implementation for this platform
let watcher = notify::new(tx);

// Watch files!
watcher.watch(Path::new("/path/to/foo"));

// Receive events!
println!("{}", rx.recv());
```

## Platforms

- Linux / Android: inotify
- All platforms: polling

### Todo

- Windows: ReadDirectoryChangesW
- OS X: FSEvents
- BSD / OS X / iOS: kqueue
- Solaris 11: FEN

### Tests

Nothing is tested yet.

## Origins

Inspired by Go's [fsnotify](https://github.com/go-fsnotify/fsnotify), born out of need for [cargo watch](https://github.com/passcod/cargo-watch), and general frustration at the non-existence of C/Rust cross-platform notify libraries.

Written from scratch by [Félix Saparelli](https://passcod.name), and released in the Public Domain.
