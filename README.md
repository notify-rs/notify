# Notify

[![» Crate](https://flat.badgen.net/crates/v/notify)][crate]
[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]
[![» CI](https://flat.badgen.net/travis/passcod/notify/main)][build]
[![» Downloads](https://flat.badgen.net/crates/d/notify)][crate]
[![» Conduct](https://flat.badgen.net/badge/contributor/covenant/5e0d73)][coc]
[![» Public Domain](https://flat.badgen.net/badge/license/CC0-1.0/purple)][cc0]

_Cross-platform filesystem notification library for Rust._

**This is the readme for the 5.0.0-pre.1 pre-release!**

(Looking for desktop notifications instead? Have a look at [notify-rust] or
[alert-after]!)

- **wip [Guides and in-depth docs][wiki]**
- [API Documentation][docs]
- [Crate page][crate]
- [Changelog][changelog]
- Earliest supported Rust version: **1.32.0**

As used by: [alacritty], [cargo watch], [cobalt], [docket], [mdBook], [pax]
[rdiff], [rust-analyzer], [timetrack], [watchexec], [xi-editor], and others.
(Want to be added to this list? Open a pull request!)

## Why a prerelease?

It’s taking a while to bring 5.0 to the standard and featureset I wish for it,
while at the same time I have less time than ever to spend on this project. In
short, don’t expect 5.0.0 before Q4 2019. I am aware, though, that people want
to use the features that are finished so far. This is what this prerelease is.

It has all the fixes and implemented features so far, with the new `Event`
interface for the "debounced" watcher, but keeping the previous events for the
immediate (previously known as "raw") watcher. It is fairly stable in terms of
functionality, and the debounced (default) API is as close as its final 5.0.0
form as it can be.

The idea is to _pin_ to `=5.0.0-pre.1`, and ignore further prereleases. You’ll
get long-standing fixes compared to 4.0.x, some new features, and API stability
for the next few months.

The 4.0.x branch will continue being passively maintained during this time
though, and it’s what out there in the ecosystem right now, so it’s always an
option to go for [the latest 4.0 release].

If you want to live at the bleeding edge, you can of course track `main` or
future prereleases. Keep in mind that there will be breakage, there will be
changes, and entire features may disappear and reappear between prereleases.
It’s gonna be pretty unstable for a while.

[the latest 4.0 release]: https://github.com/passcod/notify/tree/v4.0.10#notify

<sup>(What happened to `5.0.0-pre.0`? I broke it. I'm sorry. `.1` is just like it, though.)</sup>

## Installation

```toml
[dependencies]
crossbeam-channel = "0.3.8"
notify = "=5.0.0-pre.1"
```

## Usage

```rust
use crossbeam_channel::unbounded;
use notify::{RecursiveMode, Result, watcher};
use std::time::Duration;

fn main() -> Result<()> {
    // Create a channel to receive the events.
    let (tx, rx) = unbounded();

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let mut watcher = watcher(tx, Duration::from_secs(2))?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch("/home/test/notify", RecursiveMode::Recursive)?;

    // This is a simple loop, but you may want to use more complex logic here,
    // for example to handle I/O.
    loop {
        match rx.recv() {
            Ok(event) => println!("changed: {:?}", event),
            Err(err) => println!("watch error: {:?}", err),
        };
    }

    Ok(())
}
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
let (tx, rx) = unbounded();
let mut watcher = immediate_watcher(tx)?;
```

### Serde

Debounced Events can be serialisable via [serde]. To enable the feature:

```toml
notify = { version = "=5.0.0-pre.1", features = ["serde"] }
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

While this current version continues to be developed and maintained, next
generation experiments and designs around the library live in the
[`next`](https://github.com/passcod/notify/tree/next) branch. There is no solid
ETA, beyond that most of it will not be released before `async`/`await` is
stabilised in Rust. For an overview and background, see [this draft
announce](https://github.com/passcod/notify/wiki/Presentation).

Instead of one large release, though, smaller components of the design, once
they have gone through revising and maturing, will be incorporated in the
`main` branch. The first large piece, a new event classification system, has
already landed.

## License

Notify is currently undergoing a transition to using the
[Artistic License 2.0][artistic] from the current [CC Zero 1.0][cc0]. A part of
the code is only under CC0, and another part, including _all new code_ since
commit [`3378ac5a`], is under _both_ CC0 and Artistic. When the code will be
entirely free of CC0 code, the license will be formally changed (and that will
incur a major version bump). As part of this, when you contribute to Notify you
currently agree to release under both.

[`3378ac5a`]: https://github.com/passcod/notify/commit/3378ac5ad5f174dfeacce6edadd7ded1a08d384e

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
[docs]: https://docs.rs/notify/5.0.0-pre.1/notify/
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
