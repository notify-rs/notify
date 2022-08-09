# Upgrading from notify v4 to v5

This guide documents changes between v4 and v5 for upgrading existing code.

Notify v5 only contains precise events. Debouncing is done by a separate crate [notify-debouncer-mini](https://github.com/notify-rs/notify/tree/main/notify-debouncer-mini).

If you've used the default debounced API, please see [here](https://github.com/notify-rs/notify/blob/main/examples/debounced.rs) for an example.

For precise events you can see [here](https://github.com/notify-rs/notify/blob/main/examples/monitor_raw.rs).

Notify v5 by default uses crossbeam-channel internally. You can disable this (required for tokio) as documented in the crate.

Plattform support in v5 now includes BSD and kqueue on macos in addition to fsevent.