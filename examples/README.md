Examples for notify and the debouncers.

### Notify

- **monitor_raw** basic example for using notify
- **async_monitor** example for using `futures::channel` to receive events in async code
- **poll_sysfs** example for observing linux `/sys` events using PollWatcher and the hashing mode
- **watcher_kind** example for detecting the kind of watcher used and running specific configurations
- **hot_reload_tide** large example for async notify using the crates tide and async-std
- **pollwatcher_scan** example using `PollWatcher::with_initial_scan` to listen for files found during initial scanning
- **pollwatcher_manual** example using `PollWatcher::poll` without automatic polling for manual triggered polling

### Notify Debouncer Full (debouncer)

- **monitor_debounced** basic usage example for the debouncer
- **debouncer_full** advanced usage accessing the internal file ID cache

### Debouncer Mini (mini debouncer)

- **debouncer_mini** basic usage example for the mini debouncer
- **debouncer_mini_custom** using the mini debouncer with a specific backend (PollWatcher)
