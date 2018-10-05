# Reporting an issue or opening a pull request?

When reporting an issue, please include **all of the following**:

 - OS/Platform name and version
 - Rust version: `rustc --version`
 - Notify version (or commit hash if building from source)

And as much of the following as you can / think is relevant:

 - If you're coming from a downstream project that uses Notify: what that
   project is, its version, and a link to the relevant issue there if any
 - Filesystem type and options
 - On Linux: Kernel version
 - On Windows: if you're running under Windows, Cygwin, Linux Subsystem
 - If you're running as a privileged user (root, System)
 - If you're running in a container, details on the runtime and overlay
 - If you're running in a VM, details on the hypervisor

When opening a pull request, you agree to [the terms of the license](./LICENSE).

After opening a pull request, the test suite will run. It is expected that **if
any failures occur** in the critical builds (there are a number of non-critical
builds run by CI that may fail, these are marked clearly in Travis), you either
fix the errors, ask for help, or note that the failures are expected with a
detailed explanation.

Also see the sections below for more.

# How to Help

Hi there! Thank you for your interest in this project.

There are a number of ways in which you can help.

 - [Running tests](#running-tests)
 - [Reviewing documentation](#reviewing-documentation) (no Rust needed!)
 - [Reproducing issues](#reproducing-issues)
 - [Upgrading dependents](#upgrading-dependents)
 - [Writing a backend](#writing-a-backend)
 - [Improving the core](#improving-the-core)

There are also some things you need to know:

### The Wiki

[The Wiki](https://github.com/passcod/notify/wiki) contains additional
documentation including tutorials, guides, descriptions of internals, and more!

### Versioning indicators

When commenting on an issue or pull request, a maintainer may post something
like this:

> [non-breaking, patch]

or

> [breaking, major]

These are **versioning indicators**! They are used to decide:

 - what to put in changelogs
 - when to merge something
 - when to release a new version
 - which version number should change

Generally, these are for maintainer use, but if you think something's wrong,
like a change being breaking when the indicator says it's not, please do
comment! Maintainers are people just like you, and we make mistakes too.

### Conduct

Generally, we try to go by [The Contributor Covenant].

[The Contributor Covenant]: https://www.contributor-covenant.org/version/1/4/code-of-conduct

However, the language in that text is geared towards larger projects with more
maintainers. This project is tiny, people-wise. There's no single email address
that can be used to contact "the project team". And the "project leadership" is
one person. None of these things mean our standards are lower! But keep them in
mind while seeking help.

Specifically, though, enforcement will be handled thusly:

1. (This should be 90% of cases.) **A simple reminder not to do the relevant
   thing.** Anyone can do this! The proper response here is to **apologise**,
   edit the comment to remove the offending thing if applicable, and not do it
   again. That's it :)

2. If the editing part above is not done and a maintainer thinks there is
   enough cause, a maintainer may do so themselves.

3. If discussion snowballs, a maintainer can and should **lock the thread**.

4. If things go beyond that, they may employ other means, e.g.
   * temporary bans
   * repo restrictions
   * permanent bans

5. In **extreme cases**, if the behaviour is systematic, severe, goes beyond
   the scope of this project, and/or endangers others, **outside help** may be
   sought. GitHub support may be involved, [leaders from the larger community]
   may be contacted.

[leaders from the larger community]: https://www.rust-lang.org/en-US/team.html#Moderation-team

## Running tests

For several reasons, the test suite only runs in CI on a limited set of
architectures and platforms. If you have:

 - a non-Tier-1 platform (Tier 1 platforms are Linux, macOS, Windows, all x86\_64), or
 - a virtual machine of the same, or
 - a Linux or macOS 64-bit computer with Docker and Rust installed,

and some time, you can help!

### If you have a non-Tier-1 platform or virtual machine

Install Rust, clone the project, then run:

```bash
cargo test --all
```

Then repeat the same with the `--release` flag appended.

If anything fails, and there is no [open issue] with the same failure for the
same platform, copy the build/test logs into a new issue, and also provide your
platform details and the SHA1 of the git commit you're on. Thank you!

[open issue]: https://github.com/passcod/notify/issues

### If you have an x86\_64 Linux or macOS with Docker and Rust

Install [cross](https://github.com/japaric/cross), clone the project, then run:

```bash
ci/cross-tests.sh
```

If any of these fail or hang, run it a second time to avoid Qemu errors, then
provide the logs of the test that failed. They are collected in `cross-logs/`.

If you have even more time, you can also run the "extra platforms" suite:

```bash
ci/cross-tests.sh extra
```

And if you want to run a test or build for a particular platform, try e.g.:

```bash
ci/cross-tests.sh build aarch64-linux-android
ci/cross-tests.sh test aarch64-linux-android
```

## Reviewing documentation

Documentation is very important to any project, and this one is no exception. A
great way to help is to review documentation! All you need to do is read and
use the documentation, and **point out things you think could be better**.

If you have some more time, you could also open a pull request with your
suggestions. You can do that right from GitHub's interface, no need to check
out the code or to even be able to run Rust.

Documentation changes will often be merged quickly, but will remain unreleased
until the next version change, unless a maintainer thinks it would be worth
releasing early for.

The documentation is made of:

 - **This document**
 - [The readme](../README.md)
 - [The API documentation](https://docs.rs/notify) (written alongside the code)
 - [The Wiki](#the-wiki)
 - [The issue and PR templates](./.github/)
 - Any other document in this project not listed here

## Reproducing issues

Because there are many different environments, platforms, setups, systems, and
combinations of all these, and that maintainers' time is limited, something
very helpful is to try to **reproduce open issues**.

Even **negative** reproductions are valuable: they might narrow the issue,
which makes it more likely a root cause will be identified. They may also
inform a fix, for example by showing that a solution may introduce a bug
elsewhere.

When reproducing issues, provide **as much environmental information as
possible** that you think may be relevant. At least the following are required:

 - OS/Platform name and version
 - Rust version
 - Notify version (or commit hash if building from source)

Other things you might want to include:

 - If you're coming from a downstream project that uses Notify: what that
   project is, its version, and a link to the relevant issue there if any
 - Filesystem type and options
 - On Linux: Kernel version
 - On Windows: if you're running under Windows, Cygwin, Linux Subsystem
 - If you're running as a privileged user (root, System)
 - If you're running in a container, details on the runtime and overlay
 - If you're running in a VM, details on the hypervisor

## Upgrading dependents

There are still many libraries, crates, and applications out there that use
**old versions** of Notify. If you know of one, you can try to encourage them
to switch to the latest version. If they have specific feedback, we'd love to
hear about it!

Something that's even more helpful is to **implement the upgrade** yourself and
offer pull requests to the projects in question. Be sure to ask or look through
their issues beforehand though, as they may already have started working on it
themselves!

## Writing a backend

Notify relies on external facilities to provide filesystem monitoring.
Generally, these are native kernel APIs, but it may be other things, too. For
example, polling relies only on being able to read filesystems. Interfacing
with one of these systems is the job of **backends**.

You can write a new backend in a matter of hours. There's an entire guide
showing you how to do so: [Writing a Backend].

Writing backends helps Notify in two ways:

1. it tests and informs improvements of the Core's backend interface; and
2. most importantly, it provides more support for different platforms.

Backends can be loaded from external crates at initialisation time, which means
Notify users can use your backend without you having to go through the more
rigorous process of getting it into Notify by default.

There are other advantages: for example, if you have a proprietary platform
that has a file monitoring solution baked in, you can write a backend to make
use of it within Notify-backed programs without publishing or publicising its
interface.

[Writing a Backend]: https://github.com/passcod/notify/wiki/Writing-a-backend

## Improving the core

The core is made of the Notify crate itself, which interfaces with backends,
processors, and user code to abstract away their differences and provide to
their needs; the built-in processors; the “core backends,” included in this
repo and available by default. The innards of Notify are what make it all work,
and this machinery can be challenging to take in and work on.

At minimum, you'll want to read [all internals articles on the Wiki][wiki-int].
These provide important details as well as a general understanding of the core.

[wiki-int]: https://github.com/passcod/notify/wiki/Internals

### Environment

Nothing special is needed to work on core. **A typical Rust environment**, with
the latest stable Rust. Rustup is recommended but not required (sometimes it is
useful to test with older Rusts, or on other channels).

### Internal docs

The core is **internally documented**. You can generate those docs using:

```
cargo doc --document-private-items
```

Alternatively, read them straight from the source.

### Branches

As an external contributor, sending a Pull Request will subject your code to a
battery of CI tests, on many platforms. When you have access to only one
platform, and limited time, that can be very useful. Feel free to **open up
Pull Requests early** on, even if you're just experimenting and simply wish for
a check. Indicate your intent clearly, though: a `WIP` or `CHECK ONLY; DO NOT
MERGE` mark will help maintainers along.

Internal contributors and maintainers can push branches to the main repo. **Any
branch that starts with `try-` will be automatically picked up** by CI, others
will be ignored. It's a useful mechanism to test without having to open up Pull
Requests for experiments, while keeping pressure off CI when not needed.

### Lints

Running `cargo fmt` is good form but never required. Fixing warnings is a noble
endeavour, but also never required. Passing `cargo clippy`, which is
intentionally set strict but not run in CI, is a bonus, but never required.

The only requirement, lint-wise, is that **the code builds and tests pass**.

Maintainers will take care of fixing trivial warnings and running the formatter
if needed. While everyone likes clean code and clean build output, features and
fixes take priority. Go for it, and don't fret the small stuff.

### Finding work

If there are no open issues with work to do, search through the code for `TODO`
and `FIXME`. If nothing else, extracting those into issues would be useful. If
details are lacking or things don't make sense, **ask**! Open an issue or make
contact out of band.

Improvements or entirely new features made unprompted are of course welcome,
but **pitch us first** by opening an issue, if you can. Some things won't fit
in Notify itself, others might already be worked on or planned, and knowing
what people are working on helps choosing what to work on. We'll also be glad
to offer feedback or ask details!

## Thank you!

Thank you for your time :) You're awesome.
