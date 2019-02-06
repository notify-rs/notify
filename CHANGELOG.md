# Changelog

## 4.0.7 (2019-01-23)
## 4.0.6 (2018-08-30)
## 4.0.5 (2018-08-29)
## 4.0.4 (2018-08-06)
## 4.0.3 (2017-11-26)

## unreleased

- FIX: Suppress events for files which have been moved and deleted if a new file in the original location is created quickly when using the debounced interface (eg. while safe-saving files)
## 4.0.2 (2017-11-03)

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
