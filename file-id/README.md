# File Id

[![» Docs](https://flat.badgen.net/badge/api/docs.rs/df3600)][docs]

A utility to read file IDs.

Modern file systems assign a unique ID to each file. On Linux and MacOS it is called an `inode number`, on Windows it is called `file index`.
Together with the `device id`, a file can be identified uniquely on a device at a given time.

Keep in mind though, that IDs may be re-used at some point.

## Example

```rust
let file_id = file_id::get_file_id(path).unwrap();

println!("{file_id:?}");
```

## Features

- `serde` for serde support, off by default

## Minimum Supported Rust Version (MSRV) Policy

We follow these MSRV rules:

- The current MSRV is **1.85**.
- MSRV bumps do NOT require a major release and may happen in minor releases.
- The MSRV may be updated when needed, but support for the current stable Rust release and the previous two stable releases (N, N-1, N-2) is always guaranteed.
  - For example, if the current stable version is 1.85, we guarantee support for 1.85, 1.84, and 1.83, so the minimum supported Rust version will be **at most** 1.83.
- MSRV is bumped only when required by dependencies or when adopting new stable Rust features.
- Every MSRV bump is documented in the release notes when it happens.

[docs]: https://docs.rs/file-id
