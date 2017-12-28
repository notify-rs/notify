# How to Help

Hi there! Thank you for your interest in this project.

There are a number of ways in which you can help.

## Running tests

One of the easiest ways to contribute is to run the full test suite locally.

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

Thank you for you time :) It's pretty awesome.
