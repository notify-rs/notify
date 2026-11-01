use super::*;
use crate::ErrorKind;
use crate::test::{ChannelConfig, channel_with_config, expected};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::mpsc;
use std::time::Duration;
use tempfile::tempdir;
use windows_sys::Win32::Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_HANDLE, SetLastError};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

#[test]
fn extended_removals_classify_preexisting_entries() {
    for recursive in [RecursiveMode::NonRecursive, RecursiveMode::Recursive] {
        let dir = tempdir().unwrap();
        let file = dir.path().join("file");
        let folder = dir.path().join("folder");
        std::fs::write(&file, b"existing").unwrap();
        std::fs::create_dir(&folder).unwrap();
        let (mut watcher, mut rx) = channel_with_config::<ReadDirectoryChangesWatcher>(
            ChannelConfig::default()
                .with_watcher_config(Config::default().with_windows_extended_information(true)),
        );
        watcher.watch(dir.path(), recursive);

        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir(&folder).unwrap();

        rx.wait_ordered_exact([
            expected(&file).remove_file(),
            expected(&folder).remove_folder(),
        ])
        .ensure_no_tail();
    }
}

#[test]
fn extended_recursive_lifecycle_preserves_paths_and_modification_kinds() {
    let dir = tempdir().unwrap();
    let slash_root = PathBuf::from(dir.path().to_str().unwrap().replace('\\', "/"));
    let folder = slash_root.join("folder");
    let file = folder.join("file");
    let renamed = folder.join("renamed");
    let (mut watcher, mut rx) = channel_with_config::<ReadDirectoryChangesWatcher>(
        ChannelConfig::default()
            .with_watcher_config(Config::default().with_windows_extended_information(true)),
    );
    watcher.watch_recursively(&slash_root);
    std::fs::create_dir(&folder).unwrap();
    rx.wait_ordered([expected(&folder).create_folder()]);
    std::fs::write(&file, b"content").unwrap();
    rx.wait_ordered([expected(&file).create_file(), expected(&file).modify_any()]);
    // An explicit timestamp update must not be mislabeled as a content change.
    let times = std::fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(times)
        .unwrap();
    rx.wait_ordered([expected(&file).modify_any()]);
    std::fs::rename(&file, &renamed).unwrap();
    rx.wait_ordered([
        expected(&file).rename_from(),
        expected(&renamed).rename_to(),
    ]);
    std::fs::remove_file(&renamed).unwrap();
    rx.wait_ordered([expected(&renamed).remove_file()]);
    std::fs::remove_dir(&folder).unwrap();
    rx.wait_ordered([expected(&folder).remove_folder()]);
}

#[test]
fn extended_single_file_filter_and_unwatch() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("file");
    std::fs::write(&file, b"existing").unwrap();
    let (mut watcher, mut rx) = channel_with_config::<ReadDirectoryChangesWatcher>(
        ChannelConfig::default().with_watcher_config(
            Config::default()
                .with_windows_extended_information(true)
                .with_event_kinds(EventKindMask::REMOVE),
        ),
    );
    watcher.watch_nonrecursively(&file);
    let sibling = dir.path().join("sibling");
    std::fs::write(&sibling, b"unwatched").unwrap();
    std::fs::remove_file(&sibling).unwrap();
    std::fs::remove_file(&file).unwrap();
    rx.wait_ordered_exact([expected(&file).remove_file()])
        .ensure_no_tail();
    watcher.watcher.unwatch(&file).unwrap();
    std::fs::write(&file, b"after unwatch").unwrap();
    std::fs::remove_file(&file).unwrap();
    assert!(rx.rx.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn unavailable_api_is_an_error_only_when_requested() {
    let error = resolve_extended_information(true, || None).unwrap_err();
    assert!(
        matches!(error.kind, ErrorKind::Io(ref error) if error.kind() == io::ErrorKind::Unsupported)
    );
    assert!(
        resolve_extended_information(false, || panic!("unexpected lookup"))
            .unwrap()
            .is_none()
    );
}

// Fixtures use the Windows wire layout rather than serializing a Rust struct with padding.
fn record(extended: bool, action: u32, attributes: u32, name: &[u16]) -> Vec<u8> {
    let header = if extended { 84 } else { 12 };
    let mut bytes = vec![0; header];
    bytes[4..8].copy_from_slice(&action.to_le_bytes());
    bytes[header - 4..header].copy_from_slice(&(name.len() as u32 * 2).to_le_bytes());
    if extended {
        bytes[56..60].copy_from_slice(&attributes.to_le_bytes());
    }
    bytes.extend(name.iter().flat_map(|ch| ch.to_le_bytes()));
    bytes
}

#[test]
fn decode_both_formats_without_struct_alignment_or_tail_padding() {
    for extended in [false, true] {
        let mut first = record(
            extended,
            FILE_ACTION_REMOVED,
            FILE_ATTRIBUTE_DIRECTORY,
            &[b'a' as u16],
        );
        let offset = first.len() as u32; // 14 or 86 bytes: only WCHAR-aligned.
        first[..4].copy_from_slice(&offset.to_le_bytes());
        let name = [b'\\' as u16, 0xd800, b'b' as u16]; // Preserve unpaired UTF-16 too.
        first.extend(record(extended, FILE_ACTION_ADDED, 0, &name));
        let (first, rest) = decode_notification(&first, extended).unwrap();
        assert_eq!(first.action, FILE_ACTION_REMOVED);
        assert_eq!(
            first.attributes,
            extended.then_some(FILE_ATTRIBUTE_DIRECTORY)
        );
        assert_eq!(first.name, "a");
        let (second, rest) = decode_notification(rest, extended).unwrap();
        assert!(rest.is_empty());
        assert_eq!(second.name, OsString::from_wide(&name[1..]));
    }
}

#[test]
fn decode_rejects_truncated_names_and_invalid_offsets_in_both_formats() {
    for extended in [false, true] {
        let valid = record(extended, FILE_ACTION_REMOVED, 0, &[b'a' as u16]);
        for end in 0..valid.len() {
            assert!(
                decode_notification(&valid[..end], extended).is_err(),
                "length={end}"
            );
        }
        let header = if extended { 84 } else { 12 };
        for name_len in [1_u32, u32::MAX] {
            let mut bytes = valid.clone();
            bytes[header - 4..header].copy_from_slice(&name_len.to_le_bytes());
            assert!(decode_notification(&bytes, extended).is_err());
        }
        for offset in [1_u32, valid.len() as u32, u32::MAX] {
            let mut bytes = valid.clone();
            bytes[..4].copy_from_slice(&offset.to_le_bytes());
            assert!(decode_notification(&bytes, extended).is_err());
        }
    }
}

#[test]
fn extended_classification_uses_entry_attributes_after_path_replacement() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("replaced");
    std::fs::create_dir(&path).unwrap();
    for action in [FILE_ACTION_ADDED, FILE_ACTION_REMOVED] {
        for directory in [false, true] {
            let notification = Notification {
                action,
                attributes: Some(
                    FILE_ATTRIBUTE_REPARSE_POINT
                        | if directory {
                            FILE_ATTRIBUTE_DIRECTORY
                        } else {
                            0
                        },
                ),
                name: OsString::from("replaced"),
            };
            let expected = match (action, directory) {
                (FILE_ACTION_ADDED, true) => EventKind::Create(CreateKind::Folder),
                (FILE_ACTION_ADDED, false) => EventKind::Create(CreateKind::File),
                (_, true) => EventKind::Remove(RemoveKind::Folder),
                (_, false) => EventKind::Remove(RemoveKind::File),
            };
            assert_eq!(notification_kind(&notification, &path), Some(expected));
            assert_eq!(
                notification_kind(&notification, &path.join("missing")),
                Some(expected)
            );
        }
    }
}

// A read-start failure with the exact API signature, for lifecycle tests without a filesystem.
unsafe extern "system" fn fail_extended_read(
    _: HANDLE,
    _: *mut c_void,
    _: u32,
    _: windows_sys::core::BOOL,
    _: u32,
    _: *mut u32,
    _: *mut OVERLAPPED,
    _: LPOVERLAPPED_COMPLETION_ROUTINE,
    _: i32,
) -> windows_sys::core::BOOL {
    unsafe { SetLastError(ERROR_INVALID_FUNCTION) };
    0
}

fn request(
    extended: bool,
) -> (
    Box<ReadDirectoryRequest>,
    mpsc::Receiver<Result<Event>>,
    Receiver<Action>,
) {
    let (event_tx, event_rx) = mpsc::channel();
    let (action_tx, action_rx) = unbounded();
    let root = PathBuf::from(r"C:\watched");
    let request = Box::new(ReadDirectoryRequest {
        event_handler: Arc::new(Mutex::new(event_tx)),
        event_kinds: EventKindMask::ALL,
        buffer: read_buffer(extended),
        handle: INVALID_HANDLE_VALUE,
        data: ReadData {
            watch_path: root.clone(),
            dir: root.clone(),
            reported_dir: root,
            file: None,
            complete_sem: INVALID_HANDLE_VALUE,
            is_recursive: false,
            separator_style: SeparatorStyle::Backslash,
            stopping: Arc::new(AtomicBool::new(false)),
            read_directory_changes_ex: extended.then_some(fail_extended_read),
        },
        action_tx,
    });
    (request, event_rx, action_rx)
}

fn semaphore() -> OwnedHandle {
    let handle = unsafe { CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
    assert!(!handle.is_null());
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

fn complete(request: Box<ReadDirectoryRequest>, error: u32, bytes: u32) {
    let mut overlapped = Box::new(OVERLAPPED::default());
    overlapped.hEvent = Box::into_raw(request).cast();
    unsafe { handle_event(error, bytes, Box::into_raw(overlapped)) };
}

#[test]
fn fatal_errors_reach_the_handler_in_both_formats() {
    for extended in [false, true] {
        let (mut request, events, actions) = request(extended);
        let sem = semaphore();
        request.data.complete_sem = sem.as_raw_handle();
        request.event_kinds = EventKindMask::empty();
        let path = request.data.watch_path.clone();
        complete(request, ERROR_INVALID_FUNCTION, 0);
        let error = events.recv().unwrap().unwrap_err();
        assert!(
            matches!(error.kind, ErrorKind::Io(ref error) if error.raw_os_error() == Some(ERROR_INVALID_FUNCTION as i32))
        );
        assert_eq!(error.paths, [path.clone()]);
        assert!(matches!(actions.recv().unwrap(), Action::Unwatch(p) if p == path));
        assert_eq!(
            unsafe { WaitForSingleObjectEx(sem.as_raw_handle(), 0, 0) },
            WAIT_OBJECT_0
        );
        assert!(events.try_recv().is_err());
    }
}

#[test]
fn access_denied_without_deletion_reports_only_the_rearm_error() {
    let dir = tempdir().unwrap();
    for extended in [false, true] {
        let (mut request, events, actions) = request(extended);
        let sem = semaphore();
        request.data.complete_sem = sem.as_raw_handle();
        request.data.dir = dir.path().to_path_buf();
        let path = request.data.watch_path.clone();

        // The directory exists and the invalid handle cannot be delete-pending. The standard
        // rearm fails with INVALID_HANDLE; the extended read stub fails with INVALID_FUNCTION.
        // Neither error is the ACCESS_DENIED supplied to the completion callback.
        complete(request, ERROR_ACCESS_DENIED, 0);

        let expected_code = if extended {
            ERROR_INVALID_FUNCTION
        } else {
            ERROR_INVALID_HANDLE
        };
        let results: Vec<_> = events.try_iter().collect();
        assert_eq!(results.len(), 1, "unexpected events: {results:?}");
        let error = results[0].as_ref().unwrap_err();
        assert!(
            matches!(&error.kind, ErrorKind::Io(error) if error.raw_os_error() == Some(expected_code as i32))
        );
        assert_eq!(error.paths, [path.clone()]);
        assert!(matches!(actions.try_recv().unwrap(), Action::Unwatch(p) if p == path));
        assert!(actions.try_recv().is_err());
        assert_eq!(
            unsafe { WaitForSingleObjectEx(sem.as_raw_handle(), 0, 0) },
            WAIT_OBJECT_0
        );
    }
}

#[test]
fn received_length_bounds_decoding_in_both_formats() {
    for extended in [false, true] {
        let (mut request, events, _) = request(extended);
        let bytes = record(extended, FILE_ACTION_REMOVED, 0, &[b'a' as u16]);
        for (dst, src) in request.buffer.iter_mut().zip(bytes.chunks(4)) {
            let mut word = [0; 4];
            word[..src.len()].copy_from_slice(src);
            *dst = u32::from_le_bytes(word);
        }
        assert!(process_notifications(&request, bytes.len() as u32 - 1).is_err());
        assert!(
            process_notifications(&request, std::mem::size_of_val(&*request.buffer) as u32 + 1)
                .is_err()
        );
        assert!(events.try_recv().is_err());
        process_notifications(&request, bytes.len() as u32).unwrap();
        let event = events.recv().unwrap().unwrap();
        assert_eq!(
            event.kind,
            EventKind::Remove(if extended {
                RemoveKind::File
            } else {
                RemoveKind::Any
            })
        );
    }
}

#[test]
fn malformed_and_overflow_completions_report_loss_even_when_rearming_fails() {
    for extended in [false, true] {
        for bytes in [0, 1] {
            let (mut request, events, actions) = request(extended);
            let sem = semaphore();
            request.data.complete_sem = sem.as_raw_handle();
            request.event_kinds = EventKindMask::empty();
            complete(request, ERROR_SUCCESS, bytes);
            let results: Vec<_> = events.try_iter().collect();
            assert_eq!(results.len(), if bytes == 0 { 2 } else { 3 });
            if bytes != 0 {
                assert!(
                    matches!(&results[0], Err(Error { kind: ErrorKind::Io(error), .. }) if error.kind() == io::ErrorKind::InvalidData)
                );
            }
            assert!(results[results.len() - 2].as_ref().unwrap().need_rescan());
            assert!(results.last().unwrap().is_err());
            assert!(matches!(actions.recv().unwrap(), Action::Unwatch(_)));
            assert_eq!(
                unsafe { WaitForSingleObjectEx(sem.as_raw_handle(), 0, 0) },
                WAIT_OBJECT_0
            );
        }
    }
}
