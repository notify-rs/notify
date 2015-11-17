extern crate libc;
extern crate kernel32;
extern crate winapi;

use winapi::{HANDLE, INVALID_HANDLE_VALUE, fileapi, winbase, winnt};
use winapi::minwinbase::{OVERLAPPED, LPOVERLAPPED};
use winapi::winerror::ERROR_OPERATION_ABORTED;
use winapi::winnt::FILE_NOTIFY_INFORMATION;

use std::collections::HashMap;
use std::mem;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::mpsc::{channel, Sender, Receiver, RecvError};
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::thread;
use std::os::raw::c_void;

use super::{Event, Error, op, Op, Watcher};

const BUF_SIZE: u32 = 16384;

struct ReadDirectoryRequest {
    tx: Sender<Event>,
    buffer: [u8; BUF_SIZE as usize],
    handle: HANDLE
}

enum Action {
    Watch(PathBuf),
    Unwatch(PathBuf),
    Stop
}

enum SelectResult {
    Timeout,
    Action(Action),
    Error(RecvError)
}

struct ReadDirectoryChangesServer {
    rx: Receiver<Action>,
    tx: Sender<Event>,
    watches: HashMap<PathBuf, HANDLE>,
}

fn oneshot_timer(ms: u32) -> Receiver<()> {
    let (tx, rx) = channel();
    thread::spawn(move || {
            thread::sleep_ms(ms);
            tx.send(());
        });
    rx
}

impl ReadDirectoryChangesServer {
    fn start(event_tx: Sender<Event>) -> Sender<Action> {
        let (action_tx, action_rx) = channel();
        thread::spawn(move || {
            let server = ReadDirectoryChangesServer {
                tx: event_tx,
                rx: action_rx,
                watches: HashMap::new()
            };
            server.run();
        });
        action_tx
    }

    fn run(mut self) {
        loop {
            let timeout = oneshot_timer(500); // TODO: dumb, spawns a thread every time

            // have to pull the result out of the select! this way, because it
            // can't parse self.rx.recv() in its arguments.
            let res = {
                let rx = &self.rx;

                select! (
                    _ = timeout.recv() => SelectResult::Timeout, // TODO: timeout error?
                    action_res = rx.recv() => {
                        match action_res {
                            Err(e) => SelectResult::Error(e),
                            Ok(action) => SelectResult::Action(action)
                        }
                    }
                )
            };

            match res {
                SelectResult::Timeout => (),
                SelectResult::Error(e) => panic!("watcher error: {:?}", e), // TODO: what to do?
                SelectResult::Action(action) => {
                    match action {
                        Action::Watch(path) => self.add_watch(path),
                        Action::Unwatch(path) => self.remove_watch(path),
                        Action::Stop => {
                            for (_, handle) in self.watches {
                                unsafe {
                                    close_handle(handle);
                                }
                            }
                            break
                        }
                    }
                }
            }

            // call sleepex with alertable flag so that our completion routine fires
            unsafe {
                kernel32::SleepEx(500, 1);
            }
        }
    }

    fn add_watch(&mut self, path: PathBuf) {
        let encoded_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle;
        unsafe {
            handle = kernel32::CreateFileW(
                encoded_path.as_ptr(),
                winnt::FILE_LIST_DIRECTORY,
                winnt::FILE_SHARE_READ | winnt::FILE_SHARE_DELETE,
                ptr::null_mut(),
                fileapi::OPEN_EXISTING,
                winbase::FILE_FLAG_BACKUP_SEMANTICS | winbase::FILE_FLAG_OVERLAPPED,
                ptr::null_mut());

            if handle == INVALID_HANDLE_VALUE {
                let _ = self.tx.send(Event {
                    path: None,
                    // TODO: Call GetLastError for better error info?
                    op: Err(Error::PathNotFound)
                });
                return;
            }
        }
        self.watches.insert(path, handle);
        start_read(&self.tx, handle);
    }

    fn remove_watch(&mut self, path: PathBuf) {
        if let Some(handle) = self.watches.remove(&path) {
            unsafe {
                close_handle(handle);
            }
        }
    }
}

unsafe fn close_handle(handle: HANDLE) {
    // TODO: Handle errors
    kernel32::CancelIo(handle);
    kernel32::CloseHandle(handle);
}

fn start_read(tx: &Sender<Event>, handle: HANDLE) {
    let mut request = Box::new(ReadDirectoryRequest {
        tx: tx.clone(),
        handle: handle,
        buffer: [0u8; BUF_SIZE as usize]
    });

    let flags = winnt::FILE_NOTIFY_CHANGE_FILE_NAME
              | winnt::FILE_NOTIFY_CHANGE_DIR_NAME
              | winnt::FILE_NOTIFY_CHANGE_ATTRIBUTES
              | winnt::FILE_NOTIFY_CHANGE_SIZE
              | winnt::FILE_NOTIFY_CHANGE_LAST_WRITE
              | winnt::FILE_NOTIFY_CHANGE_CREATION
              | winnt::FILE_NOTIFY_CHANGE_SECURITY;

    unsafe {
        let mut overlapped: Box<OVERLAPPED> = Box::new(mem::zeroed());
        // When using callback based async requests, we are allowed to use the hEvent member
        // for our own purposes

        let req_buf = request.buffer.as_mut_ptr() as *mut c_void;
        let request_p: *mut c_void = mem::transmute(request);
        overlapped.hEvent = request_p;

        // This is using an asynchronous call with a completion routine for receiving notifications
        // An I/O completion port would probably be more performant
        kernel32::ReadDirectoryChangesW(
            handle,
            req_buf,
            BUF_SIZE,
            1,  // We do want to monitor subdirectories
            flags,
            &mut 0u32 as *mut u32,  // This parameter is not used for async requests
            &mut *overlapped as *mut OVERLAPPED,
            Some(handle_event));

        mem::forget(overlapped);
    }
}

unsafe extern "system" fn handle_event(error_code: u32, _bytes_written: u32, overlapped: LPOVERLAPPED) {
    // TODO: Use Box::from_raw when it is no longer unstable
    let overlapped: Box<OVERLAPPED> = mem::transmute(overlapped);
    let request: Box<ReadDirectoryRequest> = mem::transmute(overlapped.hEvent);

    if error_code == ERROR_OPERATION_ABORTED {
        // We receive this error when the directory for this request is unwatched?
        return;
    }

    // Get the next request queued up as soon as possible
    start_read(&request.tx, request.handle);

    // The FILE_NOTIFY_INFORMATION struct has a variable length due to the variable length string
    // as its last member.  Each struct contains an offset for getting the next entry in the buffer
    let mut cur_offset: *const u8 = request.buffer.as_ptr();
    let mut cur_entry: *const FILE_NOTIFY_INFORMATION = mem::transmute(cur_offset);
    loop {
        // filename length is size in bytes, so / 2
        let len = (*cur_entry).FileNameLength as usize / 2;
        let encoded_path: &[u16] = slice::from_raw_parts((*cur_entry).FileName.as_ptr(), len);
        let path = PathBuf::from(OsString::from_wide(encoded_path));

        let op = match (*cur_entry).Action {
            winnt::FILE_ACTION_ADDED => op::CREATE,
            winnt::FILE_ACTION_REMOVED => op::REMOVE,
            winnt::FILE_ACTION_MODIFIED => op::WRITE,
            winnt::FILE_ACTION_RENAMED_OLD_NAME | winnt::FILE_ACTION_RENAMED_NEW_NAME => op::RENAME,
            _ => Op::empty()
        };

        let _ = request.tx.send(Event {
            path: Some(path),
            op: Ok(op)
        });

        if (*cur_entry).NextEntryOffset == 0 {
            break;
        }
        cur_offset = cur_offset.offset((*cur_entry).NextEntryOffset as isize);
        cur_entry = mem::transmute(cur_offset);
    }

    // TODO: need to free request and overlapped
}

pub struct ReadDirectoryChangesWatcher {
    tx: Sender<Action>
}

impl Watcher for ReadDirectoryChangesWatcher {
    fn new(event_tx: Sender<Event>) -> Result<ReadDirectoryChangesWatcher, Error> {
        let action_tx = ReadDirectoryChangesServer::start(event_tx);

        return Ok(ReadDirectoryChangesWatcher {
            tx: action_tx
        });
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        // TODO: Add SendError to notify::Error and use try!(...)?
        self.tx.send(Action::Watch(path.as_ref().to_path_buf()));
        Ok(())
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        // TODO: Add SendError to notify::Error and use try!(...)?
        self.tx.send(Action::Unwatch(path.as_ref().to_path_buf()));
        Ok(())
    }
}

impl Drop for ReadDirectoryChangesWatcher {
    fn drop(&mut self) {
        let _ = self.tx.send(Action::Stop);
    }
}
