# Changelog

## 4.0.17 (2021-05-13)

- FIX: Don't crash on macos when creating & deleting folders in rapid succession [#303]

[#303]: https://github.com/notify-rs/notify/pull/303

## 4.0.16 (2021-04-14)

- FIX: Report events promptly on Linux, even when many occur in rapid succession. [#268]
- FIX: Fix leaks on Windows and debounce module. [#288]
- FIX: Display proper error message when reaching inotify limits on linux. [#290]

[#268]: https://github.com/notify-rs/notify/pull/268
[#288]: https://github.com/notify-rs/notify/pull/288
[#290]: https://github.com/notify-rs/notify/pull/290

## 4.0.15 (2020-01-07)

- DEPS: Update inotify to 0.7.
- DEPS(DEV): Replace tempdir with tempfile since tempdir is deprecated.
- DEPS: Update winapi to 0.3 and remove kernel32-sys. [#232]

[#232]: https://github.com/notify-rs/notify/pull/232

## 4.0.14 (2019-10-17)

- FIX: Fix deadlock in debouncer. [#210], [`6ccf3e8d`]

[#202]: https://github.com/notify-rs/notify/pull/210
[`22e40f5e`]: https://github.com/notify-rs/notify/commit/6ccf3e8d87f7643dde65b1f6abd911f1c971e569

## 4.0.13 (2019-09-01)

- FIX: Undo filetime pin. [#202], [`22e40f5e`]
- META: Project is abandoned.

[#202]: https://github.com/notify-rs/notify/issues/202
[`22e40f5e`]: https://github.com/notify-rs/notify/commit/22e40f5e4cb2a23528f169fc92015f935edc1c55

## 4.0.12 (2019-05-22)

- FIX: Implement `Sync` for PollWatcher to support FreeBSD. [#198]
- DEPS: Peg filetime to 1.2.5 to maintain rustc 1.26.1 compatibility. [#199]

[#198]: https://github.com/notify-rs/notify/issues/198
[#199]: https://github.com/notify-rs/notify/issues/199

## 4.0.11 (2019-05-08)

- DEPS: \[macOS\] Upgrade fsevent to 0.4. [#196]

[#196]: https://github.com/notify-rs/notify/issues/196

## 4.0.10 (2019-03-07)

- FIX: Panic caused by a clock race. [#182]
- DOCS: Add xi to Readme showcase. [`e6f09441`]

[#182]: https://github.com/notify-rs/notify/issues/182
[`e6f09441`]: https://github.com/notify-rs/notify/commit/e6f0944165551fa2ed9ad70e3e11d8b14186fc0a

## 4.0.9 (2019-02-09)

- FIX: High CPU usage in some conditions when using debouncing. [#177], [#178], coming from [rust-analyzer/#556]

[#177]: https://github.com/notify-rs/notify/issues/177
[#178]: https://github.com/notify-rs/notify/issues/178
[rust-analyzer/#556]: https://github.com/rust-analyzer/rust-analyzer/issues/556

## 4.0.8 (2019-02-06)

- DOCS: Mention hotwatch as alternative API. [#175], [`34775f26`]
- DEPS: \[Linux\] Disable `stream` feature for inotify. [#176], [`e729e279`]
- DOCS: Add dates to releases in changelog. [`cc621398`]
- DOCS: Backfill changelog: 4.0.2 to 4.0.7. [`6457f697`]
- DOCS: Backfill changelog: 0.0.1 to 2.6.0. [`d34e6ee7`]

[#175]: https://github.com/notify-rs/notify/issues/175
[`34775f26`]: https://github.com/notify-rs/notify/commit/34775f2695ec236fabc79f2c938e12e4cd54047b
[#176]: https://github.com/notify-rs/notify/issues/176
[`e729e279`]: https://github.com/notify-rs/notify/commit/e729e279f0721c4a5729e725a7cd5e4d761efb58
[`cc621398`]: https://github.com/notify-rs/notify/commit/cc621398e56e2257daf5816e8c2bb01ca79e8ddb
[`6457f697`]: https://github.com/notify-rs/notify/commit/6457f6975a9171483d531fcdafb956d2ee334d55
[`d34e6ee7`]: https://github.com/notify-rs/notify/commit/d34e6ee70df9b4905cbd04fe1a2b5770a9d2a4d4


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

[#165]: https://github.com/notify-rs/notify/issues/165
[#166]: https://github.com/notify-rs/notify/issues/166
[`8310b2cc`]: https://github.com/notify-rs/notify/commit/8310b2ccf68382548914df6ffeaf45248565b9fb
[`0b4c8400`]: https://github.com/notify-rs/notify/commit/0b4c840091f5b3ebd3262d7109308828800dc976
[#167]: https://github.com/notify-rs/notify/issues/167
[#170]: https://github.com/notify-rs/notify/issues/170
[#174]: https://github.com/notify-rs/notify/issues/174
[`eab75118`]: https://github.com/notify-rs/notify/commit/eab75118464dc5d0d48dce31ab7a8e07d7e68d80
[`50924cd6`]: https://github.com/notify-rs/notify/commit/50924cd676c8bce877634e32260ef3872f2feccb
[`ecd686ba`]: https://github.com/notify-rs/notify/commit/ecd686bab604442c315c114e536bdc310a9413b1
[`20c40f99`]: https://github.com/notify-rs/notify/commit/20c40f99ad042fba5abf36f65e9ee598562744d8
[`c00da47c`]: https://github.com/notify-rs/notify/commit/c00da47ce63815972ef7c4bafd3b8c2c11b8b0de


## 4.0.6 (2018-08-30)

- FIX: Add some consts to restore semver compatibility. [`6d4f1ab9`]

[`6d4f1ab9`]: https://github.com/notify-rs/notify/commit/6d4f1ab9af76ecfc856f573a3f5584ddcfe017df


## 4.0.5 (2018-08-29)

- DEPS: Update winapi (0.3), mio (0.6), inotify (0.6), filetime (0.2), bitflags (1.0). [#162]
- SEMVER BREAK: The bitflags upgrade introduced a breaking change to the API.

[#162]: https://github.com/notify-rs/notify/issues/162


## 4.0.4 (2018-08-06)

- Derive various traits for `RecursiveMode`. [#148]
- DOCS: Add docket to Readme showcase. [#154]
- DOCS: [Rename OS X to macOS](https://www.wired.com/2016/06/apple-os-x-dead-long-live-macos/). [#156]
- FIX: \[FreeBSD / Poll\] Release the lock while the thread sleeps (was causing random hangs). [#159]

[#148]: https://github.com/notify-rs/notify/issues/148
[#154]: https://github.com/notify-rs/notify/issues/154
[#156]: https://github.com/notify-rs/notify/issues/156
[#159]: https://github.com/notify-rs/notify/issues/159


## 4.0.3 (2017-11-26)

- FIX: \[macOS\] Concurrency-related FSEvent crash. [#132]
- FIX: \[macOS\] Deadlock due to race in FsEventWatcher. [#118], [#134]
- DEPS: Update walkdir to 2.0. [`fbffef24`]

[#118]: https://github.com/notify-rs/notify/issues/118
[#132]: https://github.com/notify-rs/notify/issues/132
[#134]: https://github.com/notify-rs/notify/issues/134
[`fbffef24`]: https://github.com/notify-rs/notify/commit/fbffef244726aae6e8a98e33ecb77a66274db91b


## 4.0.2 (2017-11-03)

- FIX: Suppress events for files which have been moved and deleted if a new file in the original location is created quickly when using the debounced interface (eg. while safe-saving files) [#129]

[#129]: https://github.com/notify-rs/notify/issues/129


## 4.0.1 (2017-03-25)

- FIX: \[Linux\] Detect moves if two connected move events are split between two mio polls


## 4.0.0 (2017-02-07)

- CHANGE: \[Linux\] Update dependency to inotify 0.3.0.
- FIX: \[macOS\] `.watch()` panics on macOS when the target doesn't exist. [#105]

[#105]: https://github.com/notify-rs/notify/issues/105

## (start work on vNext) (2016-12-29)

## 3.0.1 (2016-12-05)

- FIX: \[macOS\] Fix multiple panics in debounce module related to move events. [#99], [#100], [#101]

[#99]: https://github.com/notify-rs/notify/issues/99
[#100]: https://github.com/notify-rs/notify/issues/100
[#101]: https://github.com/notify-rs/notify/issues/101


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

[#60]: https://github.com/notify-rs/notify/issues/60
[#61]: https://github.com/notify-rs/notify/issues/61
[#63]: https://github.com/notify-rs/notify/issues/63
[#88]: https://github.com/notify-rs/notify/issues/88
[#90]: https://github.com/notify-rs/notify/issues/90
[#93]: https://github.com/notify-rs/notify/issues/93


## 2.6.3 (2016-08-05)

- FIX: \[macOS\] Bump `fsevents` version. [#91]

[#91]: https://github.com/notify-rs/notify/issues/91


## 2.6.2 (2016-07-05)

- FEATURE: \[macOS\] Implement Send and Sync for FsWatcher. [#82]
- FEATURE: \[Windows\] Implement Send and Sync for ReadDirectoryChangesWatcher. [#82]
- DOCS: Add example to monitor a given file or directory. [#77]

[#77]: https://github.com/notify-rs/notify/issues/77
[#82]: https://github.com/notify-rs/notify/issues/82


## 2.6.1 (2016-06-09)

- FIX: \[Linux\] Only register _directories_ for watching. [#74]
- DOCS: Update Readme example code. [`ccfb54be`]

[`ccfb54be`]: https://github.com/notify-rs/notify/commit/ccfb54bed3df7c4c5e0058566f10232e92b526a4
[#74]: https://github.com/notify-rs/notify/issues/74


## 2.6.0 (2016-06-06)

- Fix clippy lints. [#57]
- Run formatter. [#59]
- Fix warnings. [#58]
- FEATURE: Add `Result` alias. [#72]
- DEPS: Update inotify (0.2). [#49], [#70]
- DOCS: Write API docs. [#65]
- FEATURE: \[Linux\] Add op::IGNORED. [#73]
- Simplify Cargo.toml. [#75]
- FEATURE: Implement std::Error for our Error type. [#71]

[#57]: https://github.com/notify-rs/notify/issues/57
[#59]: https://github.com/notify-rs/notify/issues/59
[#58]: https://github.com/notify-rs/notify/issues/58
[#72]: https://github.com/notify-rs/notify/issues/72
[#49]: https://github.com/notify-rs/notify/issues/49
[#70]: https://github.com/notify-rs/notify/issues/70
[#65]: https://github.com/notify-rs/notify/issues/65
[#73]: https://github.com/notify-rs/notify/issues/73
[#75]: https://github.com/notify-rs/notify/issues/75
[#71]: https://github.com/notify-rs/notify/issues/71


## 2.5.5 (2016-03-27)

- DOCS: Explain an FSEvent limitation. [#51]
- DOCS: Clean up example code. [#52]
- RELENG: \[macOS\] Support i686. [#54]
- FEATURE: Implement `Display` on `Error`. [#56]

[#51]: https://github.com/notify-rs/notify/issues/51
[#52]: https://github.com/notify-rs/notify/issues/52
[#54]: https://github.com/notify-rs/notify/issues/54
[#56]: https://github.com/notify-rs/notify/issues/56


## 2.5.4 (2016-01-23)

- META: Remove all `*` specifiers to comply with Crates.io policy. [`9f44843f`]

[`9f44843f`]: https://github.com/notify-rs/notify/commit/9f44843f2e70b2e1fcb3ef8b0834692fe75a99a6


## 2.5.3 (2015-12-25)

- RELENG: \[Linux\] Support i686. [#46]

[#46]: https://github.com/notify-rs/notify/issues/46


## 2.5.2 (2015-12-21)

- META: Fix AppVeyor build. [#43], [#45]
- FIX: \[Linux\] Use a mio loop instead of handmade. [#40]
- DEPS: Replace walker by walkdir (0.1). [#44]

[#43]: https://github.com/notify-rs/notify/issues/43
[#45]: https://github.com/notify-rs/notify/issues/45
[#40]: https://github.com/notify-rs/notify/issues/40
[#44]: https://github.com/notify-rs/notify/issues/44


## 2.5.1 (2015-12-05)

- META: Update Code of Conduct. [`c963bdf0`]
- RELENG: Support musl. [#42]

[`c963bdf0`]: https://github.com/notify-rs/notify/commit/c963bdf0a94d951f5d11ca2a691eeb42746e721b
[#42]: https://github.com/notify-rs/notify/issues/42


## 2.5.0 (2015-11-29)

- FEATURE: Add Windows backend. [#39]
- META: Add AppVeyor CI. [`304473c3`]

[`304473c3`]: https://github.com/notify-rs/notify/commit/304473c32a76ec60bbcb20a1d673fa7c5879767d
[#39]: https://github.com/notify-rs/notify/issues/39


## 2.4.1 (2015-11-06)

- FIX: \[macOS\] Race condition in FSEvent. [#33]


## 2.4.0 (2015-10-25)

- FIX: \[macOS\] Stop segfault when watcher is moved. [#33], [#35]
- FEATURE: Add `::with_delay` to poll. [#34]

[#33]: https://github.com/notify-rs/notify/issues/33
[#35]: https://github.com/notify-rs/notify/issues/35
[#34]: https://github.com/notify-rs/notify/issues/34


## 2.3.3 (2015-10-08)

- FIX: Comply with Rust RFC 1214, adding `Sized` bound to trait. [#32]

[#32]: https://github.com/notify-rs/notify/issues/32


## 2.3.2 (2015-09-08)

- META: Use Travis CI Linux containers, macOS builds. [`7081297d`]
- FIX: \[macOS\] Symlinks and broken tests. [#27]

[`7081297d`]: https://github.com/notify-rs/notify/commit/7081297de6c557484e4cc7fbf8b2837a7d408870
[#27]: https://github.com/notify-rs/notify/issues/27


## 2.3.1 (2015-08-24)

- FIX: Move paths instead of borrowing. [`3340d740`]

[`3340d740`]: https://github.com/notify-rs/notify/commit/3340d7401230be5f5ed59956b29f8db3a1c12d1c


## 2.3.0 (2015-07-29)

- FEATURE: Use `AsRef<Path>` instead of `Path` in signatures. [#25]

[#25]: https://github.com/notify-rs/notify/issues/25


## 2.2.0 (2015-07-12)

- FEATURE: \[Linux\] Support watching single file. [#22]
- META: Change release commit message style to be just the version, instead of "Cut ${version}".

[#22]: https://github.com/notify-rs/notify/issues/22


## 2.1.0 (2015-06-26)

- META: Add Code of Conduct. [`4b88f7d9`]
- FEATURE: Restore Poll backend. [#12]
- FIX: \[Linux\] Inverse op::WRITE and op::REMOVE. [#18]
- DEPS: \[macOS\] Use fsevent (0.2). [#14]

[`4b88f7d9`]: https://github.com/notify-rs/notify/commit/4b88f7d9fcc4c41bb942c46c792f64afc848db2c
[#12]: https://github.com/notify-rs/notify/issues/12
[#18]: https://github.com/notify-rs/notify/issues/18
[#14]: https://github.com/notify-rs/notify/issues/14


## 2.0.0 (2015-06-09)

- RUST: 1.0 is out! Use stable, migrate to walker crate. [`0b127a38`]
- FEATURE: \[macOS\] FSEvent backend. [#13]
- BREAKING: Remove Poll backend. [`92936460`]

[`0b127a38`]: https://github.com/notify-rs/notify/commit/0b127a383072b0136bb44f74d5580abae01e7627
[`92936460`]: https://github.com/notify-rs/notify/commit/92936460070d4dd44090fc9e3b4c4150c2ef434c
[#13]: https://github.com/notify-rs/notify/issues/13


## 1.2.2 (2015-05-09)

- RUST: Update to latest, migrate to bitflags crate. [#11]

[#11]: https://github.com/notify-rs/notify/issues/11


## 1.2.1 (2015-04-03)

- RUST: Update to latest, thread upgrades. [#9]

[#9]: https://github.com/notify-rs/notify/issues/9


## 1.2.0 (2015-03-05)

- FEATURE: Provide full path to file that caused an event. [#8]

[#8]: https://github.com/notify-rs/notify/issues/8


## 1.1.3 (2015-02-18)

- META: Add Travis CI. [`2c865803`]
- DOCS: Build using Rust-CI. [`9c7dd960`]
- RUST: Update to latest, upgrade to new IO. [#5]
- FIX: \[Linux\] Keep watching when there are no events received. [#6]

[`2c865803`]: https://github.com/notify-rs/notify/commit/2c865803ba9d04227661e3cd320732da22526634
[`9c7dd960`]: https://github.com/notify-rs/notify/commit/9c7dd960f3f4e635046b6a1dd4b847c69dfd4f94
[#5]: https://github.com/notify-rs/notify/issues/5
[#6]: https://github.com/notify-rs/notify/issues/6


## 1.1.2 (2015-02-03)

- RUST: Update to latest, using `old_io`. [`116af0c4`]

[`116af0c4`]: https://github.com/notify-rs/notify/commit/116af0c4e00b5d5b268a9d69bba772cc1f2f67fa


## 1.1.1 (2015-01-06)

- RUST: Update to latest and fix changes to channels. [`327075c2`]

[`327075c2`]: https://github.com/notify-rs/notify/commit/327075c2ecd1d0c4123e9979aeebde4248015ef6


## 1.1.0 (2015-01-06)

- FEATURE: \[Linux\] Recursive watch. [#2]

[#2]: https://github.com/notify-rs/notify/issues/2


## 1.0.5 (2015-01-03)

- RELENG: Publish on crates.io. [`6f7d38a9`]
- RUST: Update to latest. [`fd78d0e4`]
- FIX: \[Linux\] Stop panic when inotify backend is dropped. [`6f7d38a9`]

[`6f7d38a9`]: https://github.com/notify-rs/notify/commit/6f7d38a94aead30c2f059bb6bea8bdcda542d4af
[`fd78d0e4`]: https://github.com/notify-rs/notify/commit/fd78d0e4b988d92d1be81e05e1812432ad476149


## 1.0.4 (2014-12-23)

- RUST: Update to latest. [`d45c954c`]

[`d45c954c`]: https://github.com/notify-rs/notify/commit/d45c954c03f2d4d948e252abdcf7136a139b378b


## 1.0.3 (2014-12-23)

- DEPS: Update inotify (0.1). [`fcda20bb`]

[`fcda20bb`]: https://github.com/notify-rs/notify/commit/fcda20bb17e9b2b30645350c57d9bfe2ce9b78d9


## 1.0.2 (2014-12-21)

- FIX: Build on non-Linux platforms. [`55c8d7b0`]

[`55c8d7b0`]: https://github.com/notify-rs/notify/commit/55c8d7b0bb7ae767b3032ccb57571de5d506e842


## 1.0.1 (2014-12-20)

- DOCS: Add example code to Readme. [`f14e83f3`]
- META: Add cargo metadata. [`f14e83f3`]

[`f14e83f3`]: https://github.com/notify-rs/notify/commit/f14e83f33df33f362f151525bce768911933d0dd


## 1.0.0 (2014-12-20)

- FEATURE: Invent Notify.
- FEATURE: \[Linux\] inotify backend. [`d4b61fd2`]
- FEATURE: Poll backend. [`b24a5339`]
- FEATURE: Recommended watcher selection mechanism. [`a9c60ebf`]

[`d4b61fd2`]: https://github.com/notify-rs/notify/commit/d4b61fd29c82880c473f20a2d7119977817530e0
[`b24a5339`]: https://github.com/notify-rs/notify/commit/b24a5339680792cbd7b4f25ad7ec23a04b2eba57
[`a9c60ebf`]: https://github.com/notify-rs/notify/commit/a9c60ebf0fcc630bd745dc7e5106a24311c5f1bf


## 0.0.1 (2014-12-11)

- Empty release (no code)
