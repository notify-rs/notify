# v3.0.0


#### Features

* **poll watcher:** Implement CREATE and DELETE for PollWatcher


#### Breaking Changes

* Add RecursiveMode switch to Watcher::watch(..) (closes [#60](https://github.com/passcod/rsnotify/issues/60), [#61](https://github.com/passcod/rsnotify/issues/61))


### v2.6.2


#### Features

* Implement Send and Sync for OSX's FsWatcher and Windows' ReadDirectoryChangesWatcher (closes [#82](https://github.com/passcod/rsnotify/issues/82))
* Add example that monitors a given file or directory


### v2.6.1


#### Bug Fixes

* **inotify:** Only register directories for watching
