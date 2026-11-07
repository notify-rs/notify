use super::extended_information_tests::{complete, request};
use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn server(extended: bool) -> (ReadDirectoryChangesServer, mpsc::Receiver<Result<Event>>) {
    let (event_tx, event_rx) = mpsc::channel();
    let (tx, rx) = unbounded();
    let (meta_tx, _) = unbounded();
    let (cmd_tx, _) = unbounded();
    let semaphore = unsafe { CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
    assert!(!semaphore.is_null());
    (
        ReadDirectoryChangesServer {
            tx,
            rx,
            event_handler: Arc::new(Mutex::new(event_tx)),
            event_kinds: EventKindMask::ALL,
            meta_tx,
            cmd_tx,
            watches: HashMap::new(),
            directories: HashMap::new(),
            file_directories: HashMap::new(),
            next_directory_id: 0,
            wakeup_sem: Arc::new(WakeupSem(semaphore)),
            read_directory_changes_ex: resolve_extended_information(
                extended,
                lookup_read_directory_changes_ex,
            )
            .unwrap(),
        },
        event_rx,
    )
}

// These tests run the server on the test thread. Pump its APCs until the expected subscriptions
// have received events, then drain the rest of the batch to check for unintended recipients.
fn receive(
    server: &ReadDirectoryChangesServer,
    events: &mpsc::Receiver<Result<Event>>,
    ids: &[u64],
) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut received = Vec::new();
    while !ids.iter().all(|id| {
        received
            .iter()
            .any(|event: &Event| event.user_data() == Some(*id))
    }) {
        assert!(
            Instant::now() < deadline,
            "missing events for {ids:?}: {received:?}"
        );
        unsafe { WaitForSingleObjectEx(server.wakeup_sem.0, 10, 1) };
        received.extend(events.try_iter().map(Result::unwrap));
    }
    unsafe { WaitForSingleObjectEx(server.wakeup_sem.0, 100, 1) };
    received.extend(events.try_iter().map(Result::unwrap));
    received
}

#[test]
fn shared_files_preserve_paths_user_data_and_independent_unwatch() {
    for extended in [false, true] {
        let root = tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        std::fs::write(&first, b"initial").unwrap();
        std::fs::write(&second, b"initial").unwrap();
        let slash_first = PathBuf::from(first.to_str().unwrap().replace('\\', "/"));
        let (mut server, events) = server(extended);
        server
            .add_watch(
                WatchPath::new(&slash_first, Some(1)).unwrap(),
                true,
                SeparatorStyle::Slash,
            )
            .unwrap();
        server
            .add_watch(
                WatchPath::new(&second, Some(2)).unwrap(),
                false,
                SeparatorStyle::Backslash,
            )
            .unwrap();
        assert_eq!(server.directories.len(), 1);
        let id = server.watches[&first].directory_id;
        assert!(!server.directories[&id].is_recursive);

        std::fs::write(root.path().join("unwatched"), b"ignored").unwrap();
        std::fs::write(&first, b"changed").unwrap();
        std::fs::write(&second, b"changed").unwrap();
        for event in receive(&server, &events, &[1, 2]) {
            let expected = match event.user_data() {
                Some(1) => &slash_first,
                Some(2) => &second,
                other => panic!("unexpected subscription: {other:?}"),
            };
            // Compare strings too: Windows Path equality treats the two separator styles alike.
            assert_eq!(event.paths[0].as_os_str(), expected.as_os_str());
        }

        server
            .add_watch(
                WatchPath::new(&first, Some(3)).unwrap(),
                false,
                SeparatorStyle::Backslash,
            )
            .unwrap();
        assert_eq!(server.watches[&first].directory_id, id);
        assert_eq!(
            server.watches[&first].recursive_mode,
            RecursiveMode::NonRecursive
        );
        assert_eq!(server.watches[&first].reported_path, first);
        std::fs::write(&first, b"replacement").unwrap();
        for event in receive(&server, &events, &[3]) {
            assert_eq!(event.user_data(), Some(3));
            assert_eq!(event.paths[0].as_os_str(), first.as_os_str());
        }

        assert!(server.remove_watch(first.clone()));
        assert_eq!(server.directories.len(), 1);
        // The pending shared read can still contain changes to the removed subscription.
        std::fs::write(&first, b"unwatched").unwrap();
        std::fs::write(&second, b"still watched").unwrap();
        for event in receive(&server, &events, &[2]) {
            assert_eq!(event.user_data(), Some(2));
            assert_eq!(event.paths, std::slice::from_ref(&second));
        }
        assert!(server.remove_watch(second.clone()));
        assert!(server.watches.is_empty());
        assert!(server.directories.is_empty());
        assert!(server.file_directories.is_empty());
    }
}

#[test]
fn directory_and_file_subscriptions_remain_independent() {
    for extended in [false, true] {
        let root = tempdir().unwrap();
        let file = root.path().join("file");
        std::fs::write(&file, b"initial").unwrap();
        let (mut server, events) = server(extended);
        server
            .add_watch(
                WatchPath::new(root.path(), Some(1)).unwrap(),
                true,
                SeparatorStyle::Backslash,
            )
            .unwrap();
        server
            .add_watch(
                WatchPath::new(&file, Some(2)).unwrap(),
                false,
                SeparatorStyle::Backslash,
            )
            .unwrap();
        assert_eq!(server.directories.len(), 2);
        std::fs::write(&file, b"both subscriptions").unwrap();
        receive(&server, &events, &[1, 2]);
        assert!(server.remove_watch(root.path().to_path_buf()));
        std::fs::write(&file, b"only the file").unwrap();
        for event in receive(&server, &events, &[2]) {
            assert_eq!(event.user_data(), Some(2));
        }
        assert!(server.remove_watch(file));
        assert!(server.directories.is_empty());
    }
}

#[test]
fn directory_notifications_use_the_configured_separator_style() {
    for extended in [false, true] {
        for style in [SeparatorStyle::Backslash, SeparatorStyle::Slash] {
            let root = tempdir().unwrap();
            let child = root.path().join("child");
            std::fs::create_dir(&child).unwrap();
            let file = child.join("file");
            let slash_root = PathBuf::from(root.path().to_str().unwrap().replace('\\', "/"));
            let (mut server, events) = server(extended);
            server.event_kinds = EventKindMask::CREATE;
            server
                .add_watch(WatchPath::new(&slash_root, Some(1)).unwrap(), true, style)
                .unwrap();
            assert_eq!(
                server.watches[&slash_root].reported_path.as_os_str(),
                slash_root.as_os_str()
            );

            std::fs::write(&file, b"created").unwrap();
            let expected = match style {
                SeparatorStyle::Backslash => file,
                SeparatorStyle::Slash => PathBuf::from(file.to_str().unwrap().replace('\\', "/")),
            };
            for event in receive(&server, &events, &[1]) {
                assert_eq!(event.kind, EventKind::Create(CreateKind::File));
                assert_eq!(event.paths[0].as_os_str(), expected.as_os_str());
            }
            assert!(server.remove_watch(slash_root));
        }
    }
}

#[test]
fn stale_directory_cleanup_keeps_a_replacement_watch() {
    let root = tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    std::fs::write(&first, b"initial").unwrap();
    std::fs::write(&second, b"initial").unwrap();
    let (mut server, events) = server(false);
    for path in [&first, &second] {
        server
            .add_watch(
                WatchPath::new(path, Some(1)).unwrap(),
                false,
                SeparatorStyle::Backslash,
            )
            .unwrap();
    }
    let old_id = server.watches[&first].directory_id;
    server.directories[&old_id].unwatch();
    // A Watch action can precede the callback's queued cleanup action.
    server
        .add_watch(
            WatchPath::new(&first, Some(2)).unwrap(),
            false,
            SeparatorStyle::Backslash,
        )
        .unwrap();
    let new_id = server.watches[&first].directory_id;
    assert_ne!(old_id, new_id);
    server.remove_directory(old_id);
    assert_eq!(server.watches.len(), 1);
    assert_eq!(server.file_directories[root.path()], new_id);
    std::fs::write(&first, b"replacement survives").unwrap();
    receive(&server, &events, &[2]);
    server.remove_watch(first);
}

#[test]
fn shared_read_reports_loss_and_errors_for_each_subscription() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::ERROR_INVALID_FUNCTION;

    for extended in [false, true] {
        for code in [ERROR_NOTIFY_ENUM_DIR, ERROR_INVALID_FUNCTION] {
            let (mut request, events, actions) = request(extended);
            let data = Rc::get_mut(&mut request.data).unwrap();
            let sem = unsafe { CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
            assert!(!sem.is_null());
            let sem = unsafe { OwnedHandle::from_raw_handle(sem) };
            data.complete_sem = sem.as_raw_handle();
            data.event_kinds = EventKindMask::empty();
            data.targets = RefCell::new(WatchTargets::Files(
                (1..=2)
                    .map(|id| {
                        let name = OsString::from(format!("file{id}"));
                        let path = data.dir.join(&name);
                        (
                            name,
                            WatchTarget {
                                watch_path: path.clone(),
                                reported_path: path,
                                user_data: Some(id),
                                separator_style: SeparatorStyle::Backslash,
                            },
                        )
                    })
                    .collect(),
            ));
            complete(request, code, 0);
            let received: Vec<_> = events.try_iter().collect();
            assert_eq!(
                received.len(),
                if code == ERROR_NOTIFY_ENUM_DIR { 4 } else { 2 }
            );
            for id in [1, 2] {
                let errors: Vec<_> = received
                    .iter()
                    .filter_map(|result| result.as_ref().err())
                    .filter(|error| error.user_data() == Some(id))
                    .collect();
                assert_eq!(errors.len(), 1);
                assert_eq!(
                    errors[0].paths,
                    [PathBuf::from(format!(r"C:\watched\file{id}"))]
                );
                if code == ERROR_NOTIFY_ENUM_DIR {
                    assert!(received.iter().any(|result| matches!(result, Ok(event) if event.need_rescan() && event.user_data() == Some(id) && event.paths.is_empty())));
                }
            }
            assert!(matches!(actions.try_recv().unwrap(), Action::Unwatch(0)));
            assert!(actions.try_recv().is_err());
            assert_eq!(
                unsafe { WaitForSingleObjectEx(sem.as_raw_handle(), 0, 0) },
                WAIT_OBJECT_0
            );
        }
    }
}

#[test]
fn completed_reads_reuse_the_request_and_buffer() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    thread_local! {
        static SUBMISSION: Cell<(*mut OVERLAPPED, *mut c_void, u32)> = const {
            Cell::new((ptr::null_mut(), ptr::null_mut(), 0))
        };
    }

    unsafe extern "system" fn capture_read(
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
        SUBMISSION.set((overlapped, buffer, size));
        1
    }

    let (mut request, events, actions) = request(true);
    let data = Rc::get_mut(&mut request.data).unwrap();
    data.read_directory_changes_ex = Some(capture_read);
    let sem = unsafe { CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
    assert!(!sem.is_null());
    let sem = unsafe { OwnedHandle::from_raw_handle(sem) };
    data.complete_sem = sem.as_raw_handle();
    request.buffer[0] = 0x55667788;
    let original_ptr = request.buffer.as_mut_ptr().cast::<c_void>();
    let original_overlapped = &raw mut request.overlapped;
    let data = request.data.clone();

    // Zero-byte completions force a rearm and a rescan without needing fabricated records.
    complete(request, ERROR_SUCCESS, 0);
    for step in 0..8 {
        let (overlapped, buffer, size) = SUBMISSION.take();
        assert_eq!(size, 65536);
        assert_eq!(buffer as usize % align_of::<u32>(), 0);
        assert_eq!(buffer, original_ptr);
        assert_eq!(overlapped, original_overlapped);
        assert_eq!(
            unsafe { buffer.cast::<u32>().read() },
            0x55667788,
            "rearming must not clear the reused buffer"
        );
        assert!(events.try_recv().unwrap().unwrap().need_rescan());
        if step == 7 {
            data.stopping.set(true);
        }
        unsafe { handle_event(ERROR_SUCCESS, 0, overlapped) };
    }
    assert!(events.try_recv().is_err());
    assert!(actions.try_recv().is_err());
    assert_eq!(
        unsafe { WaitForSingleObjectEx(sem.as_raw_handle(), 0, 0) },
        WAIT_OBJECT_0
    );
}
