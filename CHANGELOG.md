# Changelog

## 4.0.7 (2019-01-23)

- DOCS: Document unexpected behaviour around watching a tree root. [#165], [#166]
- DOCS: Remove v2 documentation. [`8310b2cc`]
- TESTS: Change how tests are skipped. [`0b4c8400`]
- DOCS: Add timetrack to Readme showcase. [#167]
- META: Change commit message style: commits are now prefixed by a `[topic]`.
- FIX: Make sure debounced watcher terminates. [#170]
- FIX: \[Linux\] Remove thread wake-up on timeout (introduced in 4.0.5 by error). [#174]
- FIX: Restore compatibility with Rust before 1.30.0. [`eab75118`] 
- META: Enforce compatibility with Rust 1.26.1 via CI. [`50924cd6`]
- META: Add maintenance status badge. [`ecd686ba`]
- DOCS: Freeze v4 branch (2018-10-05) [`8310b2cc`] — and subsequently unfreeze it. (2019-01-19) [`20c40f99`], [`c00da47c`]

[#165]: https://github.com/passcod/notify/issues/165
[#166]: https://github.com/passcod/notify/issues/166
[`8310b2cc`]: https://github.com/passcod/notify/commit/8310b2ccf68382548914df6ffeaf45248565b9fb
[`0b4c8400`]: https://github.com/passcod/notify/commit/0b4c840091f5b3ebd3262d7109308828800dc976
[#167]: https://github.com/passcod/notify/issues/167
[#170]: https://github.com/passcod/notify/issues/170
[#174]: https://github.com/passcod/notify/issues/174
[`eab75118`]: https://github.com/passcod/notify/commit/eab75118464dc5d0d48dce31ab7a8e07d7e68d80
[`50924cd6`]: https://github.com/passcod/notify/commit/50924cd676c8bce877634e32260ef3872f2feccb
[`ecd686ba`]: https://github.com/passcod/notify/commit/ecd686bab604442c315c114e536bdc310a9413b1
[`20c40f99`]: https://github.com/passcod/notify/commit/20c40f99ad042fba5abf36f65e9ee598562744d8
[`c00da47c`]: https://github.com/passcod/notify/commit/c00da47ce63815972ef7c4bafd3b8c2c11b8b0de


## 4.0.6 (2018-08-30)

- FIX: Add some consts to restore semver compatibility. [`6d4f1ab9`]

[`6d4f1ab9`]: https://github.com/passcod/notify/commit/6d4f1ab9af76ecfc856f573a3f5584ddcfe017df


## 4.0.5 (2018-08-29)

- DEPS: Update winapi (0.3), mio (0.6), inotify (0.6), filetime (0.2), bitflags (1.0). [#162]
- SEMVER BREAK: The bitflags upgrade introduced a breaking change to the API.

[#162]: https://github.com/passcod/notify/issues/162


## 4.0.4 (2018-08-06)

- Derive various traits for `RecursiveMode`. [#148]
- DOCS: Add docket to Readme showcase. [#154]
- DOCS: [Rename OS X to macOS](https://www.wired.com/2016/06/apple-os-x-dead-long-live-macos/). [#156]
- FIX: \[FreeBSD / Poll\] Release the lock while the thread sleeps (was causing random hangs). [#159]

[#148]: https://github.com/passcod/notify/issues/148
[#154]: https://github.com/passcod/notify/issues/154
[#156]: https://github.com/passcod/notify/issues/156
[#159]: https://github.com/passcod/notify/issues/159


## 4.0.3 (2017-11-26)

- FIX: \[macOS\] Concurrency-related FSEvent crash. [#132]
- FIX: \[macOS\] Deadlock due to race in FsEventWatcher. [#118], [#134]
- DEPS: Update walkdir to 2.0. [`fbffef24`]

[#118]: https://github.com/passcod/notify/issues/118
[#132]: https://github.com/passcod/notify/issues/132
[#134]: https://github.com/passcod/notify/issues/134
[`fbffef24`]: https://github.com/passcod/notify/commit/fbffef244726aae6e8a98e33ecb77a66274db91b


## 4.0.2 (2017-11-03)

- FIX: Suppress events for files which have been moved and deleted if a new file in the original location is created quickly when using the debounced interface (eg. while safe-saving files) [#129]

[#129]: https://github.com/passcod/notify/issues/129


## 4.0.1 (2017-03-25)

- FIX: \[Linux\] Detect moves if two connected move events are split between two mio polls


## 4.0.0 (2017-02-07)

- CHANGE: \[Linux\] Update dependency to inotify 0.3.0.
- FIX: \[macOS\] `.watch()` panics on macOS when the target doesn't exist. [#105]

[#105]: https://github.com/passcod/notify/issues/105

## (start work on v5) (2016-12-29)

## 3.0.1 (2016-12-05)

- FIX: \[macOS\] Fix multiple panics in debounce module related to move events. [#99], [#100], [#101]

[#99]: https://github.com/passcod/notify/issues/99
[#100]: https://github.com/passcod/notify/issues/100
[#101]: https://github.com/passcod/notify/issues/101


## 3.0.0 (2016-10-30)

- FIX: \[Windows\] Fix watching files on Windows using relative paths. [#90]
- FEATURE: Add debounced event notification interface. [#63]
- FEATURE: \[Polling\] Implement `CREATE` and `DELETE` events for PollWatcher. [#88]
- FEATURE: \[Polling\] Add `::with_delay_ms()` constructor. [#88]
- FIX: \[macOS\] Report `ITEM_CHANGE_OWNER` as `CHMOD` events. [#93]
- FIX: \[Linux\] Emit `CLOSE_WRITE` events. [#93]
- FEATURE: Allow recursion mode to be changed. [#60], [#61] **breaking**
- FEATURE: Track move events using a cookie.
- FEATURE: \[macOS\] Return an error when trying to unwatch non-existing file or directory.
- CHANGE: \[Linux\] Remove `IGNORED` event. **breaking**
- CHANGE: \[Linux\] Provide absolute paths even if the watch was created with a relative path.

[#60]: https://github.com/passcod/notify/issues/60
[#61]: https://github.com/passcod/notify/issues/61
[#63]: https://github.com/passcod/notify/issues/63
[#88]: https://github.com/passcod/notify/issues/88
[#90]: https://github.com/passcod/notify/issues/90
[#93]: https://github.com/passcod/notify/issues/93


## 2.6.3 (2016-08-05)

- FIX: \[macOS\] Bump `fsevents` version. [#91]

[#91]: https://github.com/passcod/notify/issues/91


## 2.6.2 (2016-07-05)

- FEATURE: \[macOS\] Implement Send and Sync for FsWatcher. [#82]
- FEATURE: \[Windows\] Implement Send and Sync for ReadDirectoryChangesWatcher. [#82]
- DOCS: Add example to monitor a given file or directory.

[#82]: https://github.com/passcod/notify/issues/82


## 2.6.1 (2016-06-09)

- FIX: \[Linux\] Only register _directories_ for watching.
