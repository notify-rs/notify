# Changelog

## file-id 0.2.3 (2025-08-03)
- CHANGE: implement `AsRef<FileId>` for `FileId` [#664]

[#664]: https://github.com/notify-rs/notify/pull/664

## file-id 0.2.2 (2024-10-25)

- CHANGE: get file stats without read permission [#625]

[#625]: https://github.com/notify-rs/notify/issues/625

## file-id 0.2.1 (2023-08-21)

- CHANGE: remove serde binary experiment opt-out after it got removed [#530]

[#530]: https://github.com/notify-rs/notify/pull/530

## file-id 0.2.0 (2023-08-18)

- CHANGE: opt-out of the serde binary experiment by restricting it to < 1.0.172 [#528]
- CHANGE: license changed to dual-license of MIT OR Apache-2.0 [#520]
- CHANGE: switch from winapi to windows-sys [#494]
- CHANGE: turn FileId struct into an enum [#494]
- FEATURE: support for high resolution file ids on Windows using GetFileInformationByHandleEx [#494]

[#494]: https://github.com/notify-rs/notify/pull/494
[#520]: https://github.com/notify-rs/notify/pull/520
[#528]: https://github.com/notify-rs/notify/pull/528

## file-id 0.1.0 (2023-05-17)

Utility for reading inode numbers (Linux, MacOS) and file IDs (Windows). [#480]

[#480]: https://github.com/notify-rs/notify/pull/480
