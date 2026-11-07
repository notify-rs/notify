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
            assert_eq!(notification_kind(&notification, dir.path()), Some(expected));
            assert_eq!(
                notification_kind(&notification, &dir.path().join("missing")),
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

pub(super) fn request(
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
        overlapped: OVERLAPPED::default(),
        buffer: read_buffer(),
        data: Rc::new(ReadData {
            id: 0,
            dir: root.clone(),
            handle: INVALID_HANDLE_VALUE,
            targets: RefCell::new(WatchTargets::Directory(WatchTarget {
                watch_path: root.clone(),
                reported_path: root,
                separator_style: SeparatorStyle::Backslash,
                user_data: None,
            })),
            complete_sem: INVALID_HANDLE_VALUE,
            is_recursive: false,
            stopping: Cell::new(false),
            read_directory_changes_ex: extended.then_some(fail_extended_read),
            event_handler: Arc::new(Mutex::new(event_tx)),
            event_kinds: EventKindMask::ALL,
            action_tx,
        }),
    });
    (request, event_rx, action_rx)
}

fn semaphore() -> OwnedHandle {
    let handle = unsafe { CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
    assert!(!handle.is_null());
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

pub(super) fn complete(request: Box<ReadDirectoryRequest>, error: u32, bytes: u32) {
    let request = Box::into_raw(request);
    unsafe {
        (*request).overlapped.hEvent = request.cast();
        handle_event(error, bytes, &raw mut (*request).overlapped);
    }
}

pub(super) fn target_mut(data: &mut ReadData) -> &mut WatchTarget {
    let WatchTargets::Directory(target) = data.targets.get_mut() else {
        panic!("directory fixture")
    };
    target
}

fn process_notifications(request: &ReadDirectoryRequest, bytes_written: u32) -> io::Result<()> {
    let targets = request.data.targets.borrow();
    let pending = collect_notifications(request, bytes_written, &targets);
    dispatch_notifications(&request.data, &targets, pending)
}

#[test]
fn fatal_errors_reach_the_handler_in_both_formats() {
    for extended in [false, true] {
        let (mut request, events, actions) = request(extended);
        let data = Rc::get_mut(&mut request.data).unwrap();
        target_mut(data).user_data = Some(0);
        let sem = semaphore();
        data.complete_sem = sem.as_raw_handle();
        data.event_kinds = EventKindMask::empty();
        let path = target_mut(data).watch_path.clone();
        let id = data.id;
        complete(request, ERROR_INVALID_FUNCTION, 0);
        let error = events.recv().unwrap().unwrap_err();
        assert_eq!(error.user_data(), Some(0));
        assert!(
            matches!(error.kind, ErrorKind::Io(ref error) if error.raw_os_error() == Some(ERROR_INVALID_FUNCTION as i32))
        );
        assert_eq!(error.paths, [path]);
        assert!(matches!(actions.recv().unwrap(), Action::Unwatch(i) if i == id));
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
        let data = Rc::get_mut(&mut request.data).unwrap();
        target_mut(data).user_data = Some(u64::MAX);
        let sem = semaphore();
        data.complete_sem = sem.as_raw_handle();
        data.dir = dir.path().to_path_buf();
        let path = target_mut(data).watch_path.clone();
        let id = data.id;

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
        assert_eq!(error.user_data(), Some(u64::MAX));
        assert!(
            matches!(&error.kind, ErrorKind::Io(error) if error.raw_os_error() == Some(expected_code as i32))
        );
        assert_eq!(error.paths, [path]);
        assert!(matches!(actions.try_recv().unwrap(), Action::Unwatch(i) if i == id));
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
        target_mut(Rc::get_mut(&mut request.data).unwrap()).user_data = Some(42);
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
        assert_eq!(event.user_data(), Some(42));
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
fn masks_filter_mixed_notification_batches_in_both_formats() {
    let dir = tempdir().unwrap();
    for extended in [false, true] {
        let mut bytes = Vec::new();
        for action in [
            FILE_ACTION_ADDED,
            FILE_ACTION_REMOVED,
            FILE_ACTION_MODIFIED,
            FILE_ACTION_RENAMED_OLD_NAME,
            FILE_ACTION_RENAMED_NEW_NAME,
            u32::MAX,
        ] {
            let mut entry = record(extended, action, 0, &[b'a' as u16]);
            if action != u32::MAX {
                let offset = entry.len() as u32;
                entry[..4].copy_from_slice(&offset.to_le_bytes());
            }
            bytes.extend(entry);
        }
        let kinds = [
            EventKind::Create(if extended {
                CreateKind::File
            } else {
                CreateKind::Any
            }),
            EventKind::Remove(if extended {
                RemoveKind::File
            } else {
                RemoveKind::Any
            }),
            EventKind::Modify(ModifyKind::Any),
            EventKind::Modify(ModifyKind::Name(RenameMode::From)),
            EventKind::Modify(ModifyKind::Name(RenameMode::To)),
        ];
        for mask in [
            EventKindMask::empty(),
            EventKindMask::CREATE,
            EventKindMask::REMOVE,
            EventKindMask::MODIFY_DATA,
            EventKindMask::MODIFY_META,
            EventKindMask::MODIFY_NAME,
            EventKindMask::ALL,
        ] {
            let (mut request, events, _) = request(extended);
            let data = Rc::get_mut(&mut request.data).unwrap();
            data.dir = dir.path().to_path_buf();
            data.event_kinds = mask;
            for (dst, src) in request.buffer.iter_mut().zip(bytes.chunks(4)) {
                let mut word = [0; 4];
                word[..src.len()].copy_from_slice(src);
                *dst = u32::from_le_bytes(word);
            }
            process_notifications(&request, bytes.len() as u32).unwrap();
            let received: Vec<_> = events.try_iter().map(|event| event.unwrap().kind).collect();
            let expected: Vec<_> = kinds
                .iter()
                .copied()
                .filter(|kind| mask.matches(kind))
                .collect();
            assert_eq!(received, expected, "extended={extended}, mask={mask:?}");
        }
    }
}

#[test]
fn standard_create_classification_waits_until_dispatch() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a");
    std::fs::write(&path, b"file at decode time").unwrap();
    let (mut request, events, _) = request(false);
    Rc::get_mut(&mut request.data).unwrap().dir = dir.path().to_path_buf();
    let bytes = record(false, FILE_ACTION_ADDED, 0, &[b'a' as u16]);
    for (dst, src) in request.buffer.iter_mut().zip(bytes.chunks(4)) {
        let mut word = [0; 4];
        word[..src.len()].copy_from_slice(src);
        *dst = u32::from_le_bytes(word);
    }
    let targets = request.data.targets.borrow();
    let pending = collect_notifications(&request, bytes.len() as u32, &targets);
    assert!(events.try_recv().is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    dispatch_notifications(&request.data, &targets, pending).unwrap();
    assert_eq!(
        events.recv().unwrap().unwrap().kind,
        EventKind::Create(CreateKind::Folder)
    );
}

#[test]
fn a_valid_prefix_survives_a_malformed_record_and_failed_rearm() {
    for extended in [false, true] {
        let (mut request, events, actions) = request(extended);
        let sem = semaphore();
        let data = Rc::get_mut(&mut request.data).unwrap();
        data.complete_sem = sem.as_raw_handle();
        target_mut(data).user_data = Some(42);
        let mut bytes = record(extended, FILE_ACTION_REMOVED, 0, &[b'a' as u16]);
        let offset = bytes.len() as u32;
        bytes[..4].copy_from_slice(&offset.to_le_bytes());
        bytes.push(0); // The second record has a truncated header.
        for (dst, src) in request.buffer.iter_mut().zip(bytes.chunks(4)) {
            let mut word = [0; 4];
            word[..src.len()].copy_from_slice(src);
            *dst = u32::from_le_bytes(word);
        }
        complete(request, ERROR_SUCCESS, bytes.len() as u32);
        let received: Vec<_> = events.try_iter().collect();
        assert_eq!(received.len(), 4);
        assert_eq!(
            received[0].as_ref().unwrap().paths,
            [PathBuf::from(r"C:\watched\a")]
        );
        assert_eq!(received[0].as_ref().unwrap().user_data(), Some(42));
        assert!(
            matches!(&received[1], Err(Error { kind: ErrorKind::Io(error), .. }) if error.kind() == io::ErrorKind::InvalidData)
        );
        assert!(received[2].as_ref().unwrap().need_rescan());
        assert!(received[3].is_err());
        assert!(matches!(actions.try_recv().unwrap(), Action::Unwatch(0)));
        assert_eq!(
            unsafe { WaitForSingleObjectEx(sem.as_raw_handle(), 0, 0) },
            WAIT_OBJECT_0
        );
    }
}

#[test]
fn rearming_can_overwrite_the_buffer_before_event_delivery() {
    thread_local! {
        static NEXT_READ: Cell<*mut OVERLAPPED> = const { Cell::new(ptr::null_mut()) };
    }
    unsafe extern "system" fn overwrite_buffer(
        _: HANDLE,
        buffer: *mut c_void,
        size: u32,
        _: windows_sys::core::BOOL,
        _: u32,
        _: *mut u32,
        overlapped: *mut OVERLAPPED,
        _: LPOVERLAPPED_COMPLETION_ROUTINE,
        _: i32,
    ) -> windows_sys::core::BOOL {
        unsafe { buffer.cast::<u8>().write_bytes(0xff, size as usize) };
        NEXT_READ.set(overlapped);
        1
    }
    let (mut request, _, actions) = request(true);
    let sem = semaphore();
    let data = Rc::get_mut(&mut request.data).unwrap();
    data.complete_sem = sem.as_raw_handle();
    data.read_directory_changes_ex = Some(overwrite_buffer);
    let (tx, events) = mpsc::channel();
    data.event_handler = Arc::new(Mutex::new(move |event: Result<Event>| {
        tx.send((!NEXT_READ.get().is_null(), event)).unwrap();
    }));
    let bytes = record(true, FILE_ACTION_REMOVED, 0, &[b'a' as u16]);
    for (dst, src) in request.buffer.iter_mut().zip(bytes.chunks(4)) {
        let mut word = [0; 4];
        word[..src.len()].copy_from_slice(src);
        *dst = u32::from_le_bytes(word);
    }
    let data = request.data.clone();
    complete(request, ERROR_SUCCESS, bytes.len() as u32);
    let (rearmed, event) = events.try_recv().unwrap();
    assert!(rearmed);
    let event = event.unwrap();
    assert_eq!(event.kind, EventKind::Remove(RemoveKind::File));
    assert_eq!(event.paths, [PathBuf::from(r"C:\watched\a")]);
    assert!(events.try_recv().is_err());
    assert!(actions.try_recv().is_err());
    data.stopping.set(true);
    unsafe { handle_event(ERROR_OPERATION_ABORTED, 0, NEXT_READ.take()) };
    assert_eq!(
        unsafe { WaitForSingleObjectEx(sem.as_raw_handle(), 0, 0) },
        WAIT_OBJECT_0
    );
}

#[test]
fn malformed_and_overflow_completions_report_loss_even_when_rearming_fails() {
    for extended in [false, true] {
        for bytes in [0, 1] {
            let (mut request, events, actions) = request(extended);
            let data = Rc::get_mut(&mut request.data).unwrap();
            target_mut(data).user_data = Some(42);
            let sem = semaphore();
            data.complete_sem = sem.as_raw_handle();
            data.event_kinds = EventKindMask::empty();
            complete(request, ERROR_SUCCESS, bytes);
            let results: Vec<_> = events.try_iter().collect();
            assert_eq!(results.len(), if bytes == 0 { 2 } else { 3 });
            if bytes != 0 {
                assert!(
                    matches!(&results[0], Err(Error { kind: ErrorKind::Io(error), .. }) if error.kind() == io::ErrorKind::InvalidData)
                );
            }
            assert!(results[results.len() - 2].as_ref().unwrap().need_rescan());
            assert_eq!(
                results[results.len() - 2].as_ref().unwrap().user_data(),
                Some(42)
            );
            assert!(results.last().unwrap().is_err());
            for error in results.iter().filter_map(|result| result.as_ref().err()) {
                assert_eq!(error.user_data(), Some(42));
            }
            assert!(matches!(actions.recv().unwrap(), Action::Unwatch(_)));
            assert_eq!(
                unsafe { WaitForSingleObjectEx(sem.as_raw_handle(), 0, 0) },
                WAIT_OBJECT_0
            );
        }
    }
}
