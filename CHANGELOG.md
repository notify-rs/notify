# v3.0.0


#### Bug Fixes

* **windows watcher:** Fix watching files on windows using relative paths (closes [#90](https://github.com/passcod/rsnotify/issues/90))


#### Features

* Add debounced event notification interface (closes [#63](https://github.com/passcod/rsnotify/issues/63))
* **poll watcher:**
  * Implement CREATE and DELETE for PollWatcher
  * Add `with_delay_ms` constructor
* **fsevents watcher:** Report ITEM_CHANGE_OWNER as CHMOD events
* **inotify watcher:** Emit CLOSE_WRITE events (closes [#93](https://github.com/passcod/rsnotify/pull/93))


#### Breaking Changes

* Add RecursiveMode switch to Watcher::watch(..) (closes [#60](https://github.com/passcod/rsnotify/issues/60), [#61](https://github.com/passcod/rsnotify/issues/61))
* Track move events using a cookie
* Remove Error::NotImplemented since it wasn't used
* **fsevents watcher:** Return error when trying to unwatch non-existing file or directory
* **inotify watcher:**
  * Remove IGNORED events
  * Always emit events with absolute paths, even if a relative path is used to watch a file or directory


### v2.6.3


#### Bug Fixes

* Bump fsevents version (closes [#91](https://github.com/passcod/rsnotify/pull/91))


### v2.6.2


#### Features

* Implement Send and Sync for OSX's FsWatcher and Windows' ReadDirectoryChangesWatcher (closes [#82](https://github.com/passcod/rsnotify/issues/82))
* Add example that monitors a given file or directory


### v2.6.1


#### Bug Fixes

* **inotify:** Only register directories for watching
