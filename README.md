# Notify

[![Version](https://img.shields.io/crates/v/notify.svg?style=flat-square)][crate]
[![License: CC0](https://img.shields.io/crates/l/notify.svg?style=flat-square)][cc0]
[![Download count](https://img.shields.io/crates/d/notify.svg?style=flat-square)][crate]

<sup>**windows**:</sup> [![Windows CI](https://img.shields.io/appveyor/ci/passcod/rsnotify/next.svg?style=flat-square)][build-windows]
<sup>**\*nix**:</sup> [![\*Nix CI](https://img.shields.io/travis/passcod/notify/next.svg?style=flat-square)][build-unix]

[![Code of Conduct](https://img.shields.io/badge/contributor-covenant-5e0d73.svg?style=flat-square)](#conduct)
[![Documentation](https://img.shields.io/badge/documentation-docs.rs-df3600.svg?style=flat-square)][docs]

_Cross-platform filesystem notification library for Rust._

- [Documentation][docs]
- [Crate page][crate]
- [FAQ](/a-wiki-page-or-something?)
- [How to help](#how-to-help)

As used by: [cargo watch], [mdBook], [rdiff], [watchexec].
(Want to be added to this list? Open a pull request!)

[build-unix]: https://travis-ci.org/passcod/notify
[build-windows]: https://ci.appveyor.com/project/passcod/rsnotify
[cargo watch]: https://github.com/passcod/cargo-watch
[cc0]: https://creativecommons.org/publicdomain/zero/1.0/
[crate]: https://crates.io/crates/notify
[docs]: https://docs.rs/notify
[mdBook]: https://github.com/rust-lang-nursery/mdBook
[rdiff]: https://github.com/dyule/rdiff
[watchexec]: https://github.com/mattgreen/watchexec


## Status

**In development.**

Core decisions:

- Use Rust beta while developing, then switch to stable for first Notify beta.
- Use Tokio Reform until ecosystem stabilises.
- User-provided backends will be in 5.0.
- Runtime fallback to other methods will be in 5.0. ([#64](https://github.com/passcod/notify/issues/64))
- All Tier 1 platforms need to work as of first alpha:
  - Windows
  - Linux
  - macOS
  - polling

Backends that have good progress:

- inotify (linux)
- kqueue (BSD only, sys crate does not support macOS)
- fsevent (macOS, in a branch)
- polling (in a branch)

Backends needed but not started:

- Windows

Delayed until after release:

- All non-essential backends:
  - Remote
  - Watchman
  - fanotify
- Runtime-added backends (via dynamic .so or DLL)
- More debouncing options (possibly via feature)
- kqueue under macOS and i686 BSD ([#136](https://github.com/passcod/notify/issues/136))

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

You can also contribute financially to [passcod's Patreon][patreon].

[patreon]: https://www.patreon.com/passcod

### Conduct

This project's conduct policies are described in the
[CONTRIBUTING.md](CONTRIBUTING.md#conduct). In a few words:

- The standards described in [The Contributor Covenant] apply.
- Enforcement is explicitely defined: for most occurrences, it should be a
  simple message (from anyone, not just maintainers) not to engage in the
  behaviour.

[The Contributor Covenant]: https://www.contributor-covenant.org/version/1/4/code-of-conduct

### License

[![No Rights Reserved](https://licensebuttons.net/p/zero/1.0/88x31.png)][cc0]

This work is released to the public domain under [CC0][cc0].

Additionally, any suit or legal action relating to this work may only be
brought in New Zealand.
