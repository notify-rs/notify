# Changelog

## notify-types 2.2.0 (2026-10-28)

- FEATURE: add `debouncer_mini::DebouncedEventBatch` to distinguish path changes from rescan notifications [#1032]
- FEATURE: re-export `debouncer_full::Instant` so consumers can use the same instant type with or without the `web-time` feature [#1031]
- CHANGE: raise MSRV to 1.88
- CHANGE: update to edition 2024
- CHANGE: add `#[must_use]` annotations to `EventKind` predicates plus `Event` and `EventAttributes` getter/builder APIs

[#1031]: https://github.com/notify-rs/notify/pull/1031
[#1032]: https://github.com/notify-rs/notify/pull/1032

## notify-types 2.1.0 (2026-01-25)

- FEATURE: add `EventKindMask` for filtering filesystem events [#736]

[#736]: https://github.com/notify-rs/notify/pull/736

## notify-types 2.0.0 (2025-01-10)

- CHANGE: replace instant crate with web-time [#652] **breaking**
- CHANGE: the web-time dependency is now behind the `web-time` feature **breaking**

[#652]: https://github.com/notify-rs/notify/pull/652

## notify-types 1.0.1 (2024-12-17)

- FIX: `Event::kind` serialization with `serialization-compat-6` feature [#660]

[#660]: https://github.com/notify-rs/notify/pull/660

## notify-types 1.0.0 (2024-10-25)

New crate containing public type definitions for the notify and debouncer crates. [#559]

- CHANGE: the serialization format for events has been changed to be easier to use in environments like JavaScript;
  the old behavior can be restored using the new feature flag `serialization-compat-6` [#558] [#568] **breaking**
- CHANGE: use instant crate (which provides an `Instant` type that works in Wasm environments) [#570]

[#558]: https://github.com/notify-rs/notify/pull/558
[#559]: https://github.com/notify-rs/notify/pull/559
[#568]: https://github.com/notify-rs/notify/pull/568
[#570]: https://github.com/notify-rs/notify/pull/570
