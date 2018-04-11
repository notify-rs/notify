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
   edit your comment to remove the offending thing if needed, and not do it
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
   sought. GitHub support may be involved, leaders from the larger community
   may be contacted.

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
cargo test
cargo test -p notify-backend
cargo test -p notify-backend-poll-tree

# Any of these, depending on what your platform is
cargo test -p notify-backend-inotify   # For linux
cargo test -p notify-backend-fsevents  # For macOS
cargo test -p notify-backend-kqueue    # For *BSD
```

Then repeat the same with the `--release` flag appended.

If any of these fail, and there is no [open issue] with the same failure for
the same platform, copy the build/test logs into a new issue, and also provide
your platform details and the SHA1 of the git commit you're on. Thank you!

### If you have an x86\_64 Linux or macOS with Docker and Rust

Install [cross], clone the project, then run:

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
showing you how to do so: [docs/backend-guide.md](docs/backend-guide.md).

Writing backends helps Noitfy in two ways:

1. it tests and informs improvements of the Core's backend interface; and
2. most importantly, it provides more support for different platforms.

Backends can be loaded from external crates at initialisation time, which means
Notify users can use your backend without you having to go through the more
rigorous process of getting it into Notify by default.

There are other advantages: for example, if you have a proprietary platform
that has a file monitoring solution baked in, you can write a backend to make
use of it within Notify-backed programs without publishing or publicising its
interface.

## Improving the core

TODO.

## Thank you!

Thank you for your time :) You're awesome.
