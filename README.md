# Notify

[![Version](https://flat.badgen.net/crates/v/notify)][crate]
[![License: Artistic 2.0](https://flat.badgen.net/badge/license/Artistic%202.0/purple)][artistic]
[![Download count](https://flat.badgen.net/crates/d/notify)][crate]

[![Code of Conduct](https://flat.badgen.net/badge/contributor/covenant/5e0d73)](#conduct)
[![Documentation](https://flat.badgen.net/badge/documentation/docs.rs/df3600)][docs]

[![Appveyor CI](https://flat.badgen.net/appveyor/ci/passcod/rsnotify/next)][build-windows]
[![Travis CI](https://flat.badgen.net/travis/passcod/notify/next)][build-unix]

_Cross-platform filesystem notification library for Rust._

(Looking for desktop notifications instead? Have a look at [notify-rust] or
[alert-after]!)

- [Guides](https://github.com/passcod/notify/wiki/Guides)
- [API Documentation][docs]
- [Crate page][crate]
- [How to help](#how-to-help)
- [Acknowledgements](./ACKNOWLEDGEMENTS.md)

As used by: [cargo watch], [mdBook], [pax], [rdiff], [watchexec].
(Want to be added to this list? Open a pull request!)

[alert-after]: https://github.com/frewsxcv/alert-after
[build-unix]: https://travis-ci.org/passcod/notify
[build-windows]: https://ci.appveyor.com/project/passcod/rsnotify
[cargo watch]: https://github.com/passcod/cargo-watch
[artistic]: ./LICENSE
[crate]: https://crates.io/crates/notify
[docs]: https://docs.rs/notify
[mdBook]: https://github.com/rust-lang-nursery/mdBook
[notify-rust]: https://github.com/hoodie/notify-rust
[pax]: https://pax.js.org/
[rdiff]: https://github.com/dyule/rdiff
[watchexec]: https://github.com/mattgreen/watchexec


## Status

**In development.**

- Use Tokio Reform until ecosystem stabilises.
- Use Rust beta while developing, then switch to stable for first Notify beta.

Lists are in no particular order within sections.

Before any release

- [x] Event loop running and delivering events
- [x] Better event subscriptions (done with multiqueue)
- [x] Error reporting
- [x] Runtime fallback to other methods ([#64](https://github.com/passcod/notify/issues/64))
- [x] Less depending on Life directly, more to Manager
- [x] Being able to drop backends
- [ ] Basic public (frontend) API

Release first alpha here

- [ ] Filling in capabilities
- [ ] User-provided backends
- [ ] Being able to shutdown notify
- [ ] Basic internals documentation
- All Tier 1 platforms:
  - [ ] Windows
  - [ ] Linux
  - [ ] macOS
  - [ ] polling

Release second alpha here

- [ ] Debouncing
- [ ] Future-less API
- [ ] More extensive testing
- [ ] Full documentation

Release more alphas as the above gets in

Beta checklist:

- [ ] Freeze Event
- [ ] Freeze Backend trait
- [ ] Freeze Backend prelude
- [ ] Freeze public API
- [ ] Recheck all documentation (API, Wiki, Readme, Contributing, GH Templates)

Release beta here!

--------------------------------------------------

Backends that have good progress:

- [x] inotify (linux)
- [ ] fsevent (macOS, in a branch)
- [ ] polling (in a branch)

Backends needed but not started:

- [ ] Windows
- [ ] kqueue (current implementation disabled, will need a rewrite using mio directly) ([#136](https://github.com/passcod/notify/issues/136))

Delayed until after release:

- All non-essential backends:
  - Remote
  - Watchman
  - fanotify
  - demo Google Drive backend
  - Dynamic
- More debouncing options (possibly via feature)
- Filesystem plugins (needed for advanced remote backends)

## Installation

```toml
[dependencies]
notify = "5.0.0"
```

## Usage

```rust
```

...etc...

## Community

### How to Help

There are a number of ways in which you can help.

- [Running tests](CONTRIBUTING.md#running-tests)
- [Reviewing documentation](CONTRIBUTING.md#reviewing-documentation) (no Rust needed!)
- [Reproducing issues](CONTRIBUTING.md#reproducing-issues)
- [Upgrading dependents](CONTRIBUTING.md#upgrading-dependents)
- [Writing a backend](CONTRIBUTING.md#writing-a-backend)
- [Improving the core](CONTRIBUTING.md#improving-the-core)

You can also contribute financially with [Ko-fi] or [Patreon].

[Ko-fi]: https://ko-fi.com/passcod
[Patreon]: https://www.patreon.com/passcod

### Conduct

This project's conduct policies are described in the
[CONTRIBUTING.md](CONTRIBUTING.md#conduct). In a few words:

- The standards described in [The Contributor Covenant] apply.
- Enforcement is explicitely defined: for most occurrences, it should be a
  simple message (from anyone, not just maintainers) not to engage in the
  behaviour, but escalates from there.

[The Contributor Covenant]: https://www.contributor-covenant.org/version/1/4/code-of-conduct

### License

[Artistic License 2.0](./LICENSE), see LICENSE file for details.

Additionally, any suit or legal action relating to this work may only be
brought in New Zealand.
