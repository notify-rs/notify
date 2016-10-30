# Changelog

## 3.0.0

- FIX: \[Windows\] Fix watching files on Windows using relative paths. [#90]
- FEATURE: Add debounced event notification interface. [#63]
- FEATURE: \[Polling\] Implement `CREATE` and `DELETE` events for PollWatcher.
- FEATURE: \[Polling\] Add `::with_delay_ms()` constructor.
- FIX: \[macOS\] Report `ITEM_CHANGE_OWNER` as `CHMOD` events.
- FIX: \[Linux\] Emit `CLOSE_WRITE` events. [#93]
- FEATURE: Allow recursion mode to be changed. [#60], [#61] **breaking**
- FEATURE: Track move events using a cookie.
- FEATURE: \[macOS\] Return an error when trying to unwatch non-existing file or directory.
- CHANGE: \[Linux\] Remove `IGNORED` event. **breaking**
- CHANGE: \[Linux\] Provide absolute paths even if the watch was created with a relative path.

[#60]: https://github.com/passcod/notify/issues/60
[#61]: https://github.com/passcod/notify/issues/61
[#63]: https://github.com/passcod/notify/issues/63
[#90]: https://github.com/passcod/notify/issues/90
[#93]: https://github.com/passcod/notify/issues/93


## 2.6.3

- FIX: \[macOS\] Bump `fsevents` version. [#91]

[#91]: https://github.com/passcod/rsnotify/issues/91


## 2.6.2

- FEATURE: \[macOS\] Implement Send and Sync for FsWatcher. [#82]
- FEATURE: \[Windows\] Implement Send and Sync for ReadDirectoryChangesWatcher. [#82]
- DOCS: Add example to monitor a given file or directory.

[#82]: https://github.com/passcod/rsnotify/issues/82


## 2.6.1

- FIX: \[Linux\] Only register _directories_ for watching.
