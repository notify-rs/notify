//! Cross-platform filesystem notification library.
//!
//! TBC
//!
//! ## Public API
//!
//! This is everything that is considered the public API and taken into account for semver. It is
//! also an incomplete list of things that are _explicitly not_ taken into account for semver,
//! despite technically being available or usable publicly. These apply top-down, so a later point
//! overrides previous ones.
//!
//!  - **Everything exposed** (as `pub`) from this crate is API.
//!  - Everything exposed from **the `backend` crate** (and re-exposed here) is API.
//!  - `Debug` representations are _not_ API. Please do not rely on these being stable.
//!  - **`Display`** representations (where available) _are_ API, but only for Notify-defined types.
//!  - **Panics** are API, but _removing_ panics will not be considered breaking.
//!  - What **_traits_ the API types implement** is API. For example, dropping `Clone` from a type
//!    would be a breaking change.
//!  - The **built-in list of backends** is API, but changes to the default selection will not be
//!    breaking. However, native-platform support being _removed_ entirely from built-in for an OS
//!    will be breaking (and generally should not happen). This somewhat confusing clause is to
//!    support the case where internal selection improvements are made without bumping the major.
//!  - **Internal logging _manner_** is API, but _messages_ are not. The current manner is with the
//!    [standard logging facade](https://crates.io/crates/log), and so changing this incompatibly
//!    would be breaking. However, and as example, removing a `trace!()` call will not be breaking.
//!  - Project-wise, both **the License and the Code of Conduct** are API.
//!  - The **minimum buildable rustc** version is API, and a raise will be a major bump.
//!  - Tests, CI configuration, examples, and docs are _not_ API.
//!  - **This list** is API. How meta!
//!
//! None of these apply while Notify v5 is in alpha/beta.

#![forbid(unsafe_code)]
#![cfg_attr(feature = "cargo-clippy", deny(clippy_pedantic))]

extern crate multiqueue;
extern crate tokio;

pub extern crate notify_backend as backend;

extern crate notify_backend_poll as poll;

#[cfg(any(target_os = "linux", target_os = "android"))]
extern crate notify_backend_inotify as inotify;

// #[cfg(any(
//     target_os = "dragonfly",
//     target_os = "freebsd",
//     target_os = "netbsd",
//     target_os = "openbsd",
// ))]
// extern crate notify_backend_kqueue as kqueue;

pub mod lifecycle;
pub mod manager;
pub mod processor;
pub mod selector;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}

// TODO: add trace! everywhere
