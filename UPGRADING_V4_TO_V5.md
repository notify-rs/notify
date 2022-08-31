# Upgrading from notify v4 to v5

This guide documents changes between v4 and v5 for upgrading existing code.

## (Precise) Events & Debouncing

Notify v5 only contains precise events. Debouncing is done by a separate crate [notify-debouncer-mini]. If you relied on `RawEvent`, this got replaced by `Event`.

The old `DebouncedEvent` is completely removed. [notify-debouncer-mini] only reports an `Any` like event (named `DebouncedEvent` too), as relying on specific kinds (Write/Create/Remove) is very platform specific and [can't](https://github.com/notify-rs/notify/issues/261) [be](https://github.com/notify-rs/notify/issues/187) [guaranteed](https://github.com/notify-rs/notify/issues/272) to work, relying on a lot of assumptions. In most cases you should check anyway what exactly the state of files is, or probably re-run your application code, not relying on which event happened.

If you've used the debounced API, which was the default in v4 without `raw_`, please see [here](https://github.com/notify-rs/notify/blob/main/examples/debounced.rs) for an example using the new crate.

For an example of precise events you can look [here](https://github.com/notify-rs/notify/blob/main/examples/monitor_raw.rs).

Watchers now accept the `EventHandler` trait for event handling, allowing for callbacks and foreign channels.

## Watcher Config & Creation

All watchers only expose the `Watcher` trait, which takes an `EventHandler` and a `Config`, the latter being used to possibly initialize things that can only be specified before running the watcher. One Example would be the `compare_contents` from `PollWatcher`.

## Crate Features

Notify v5 by default uses `crossbeam-channel` internally. You can disable this as documented in the crate, this may be required for tokio users.

For macOS the kqueue backend can now be used alternatively by using the `macos_kqueue` feature.

## Platforms

Platform support in v5 now includes BSD and kqueue on macos in addition to fsevent.

[notify-debouncer-mini]: https://crates.io/crates/notify-debouncer-mini