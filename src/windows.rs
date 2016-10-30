#![warn(missing_docs)]
//! Watcher implementation for Windows' directory management APIs
//!
//! For more information see the [ReadDirectoryChangesW reference][ref].
//!
//! [ref]: https://msdn.microsoft.com/en-us/library/windows/desktop/aa363950(v=vs.85).aspx

extern crate kernel32;

use winapi::{OVERLAPPED, LPOVERLAPPED, HANDLE, INVALID_HANDLE_VALUE, INFINITE, TRUE,
             WAIT_OBJECT_0, ERROR_OPERATION_ABORTED, FILE_NOTIFY_INFORMATION, fileapi, winbase,
             winnt};

use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::mem;
use std::os::raw::c_void;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::time::Duration;
use super::{RawEvent, DebouncedEvent, Error, op, Op, Result, Watcher, RecursiveMode};
use super::debounce::{Debounce, EventTx};

const BUF_SIZE: u32 = 16384;

static mut COOKIE_COUNTER: u32 = 0;

#[derive(Clone)]
struct ReadData {
    dir: PathBuf, // directory that is being watched
    file: Option<PathBuf>, // if a file is being watched, this is its full path
    complete_sem: HANDLE,
    is_recursive: bool,
}

struct ReadDirectoryRequest {
    event_tx: Arc<Mutex<EventTx>>,
    buffer: [u8; BUF_SIZE as usize],
    handle: HANDLE,
    data: ReadData,
}

enum Action {
    Watch(PathBuf, RecursiveMode),
    Unwatch(PathBuf),
    Stop,
}

pub enum MetaEvent {
    SingleWatchComplete,
    WatcherAwakened,
}

struct WatchState {
    dir_handle: HANDLE,
    complete_sem: HANDLE,
}

struct ReadDirectoryChangesServer {
    rx: Receiver<Action>,
    event_tx: Arc<Mutex<EventTx>>,
    meta_tx: Sender<MetaEvent>,
    cmd_tx: Sender<Result<PathBuf>>,
    watches: HashMap<PathBuf, WatchState>,
    wakeup_sem: HANDLE,
}

impl ReadDirectoryChangesServer {
    fn start(event_tx: EventTx,
             meta_tx: Sender<MetaEvent>,
             cmd_tx: Sender<Result<PathBuf>>,
             wakeup_sem: HANDLE)
             -> Sender<Action> {

        let (action_tx, action_rx) = channel();
        // it is, in fact, ok to send the semaphore across threads
        let sem_temp = wakeup_sem as u64;
        thread::spawn(move || {
            let wakeup_sem = sem_temp as HANDLE;
            let server = ReadDirectoryChangesServer {
                rx: action_rx,
                event_tx: Arc::new(Mutex::new(event_tx)),
                meta_tx: meta_tx,
                cmd_tx: cmd_tx,
                watches: HashMap::new(),
                wakeup_sem: wakeup_sem,
            };
            server.run();
        });
        action_tx
    }

    fn run(mut self) {
        loop {
            // process all available actions first
            let mut stopped = false;

            while let Ok(action) = self.rx.try_recv() {
                match action {
                    Action::Watch(path, recursive_mode) => {
                        let res = self.add_watch(path, recursive_mode.is_recursive());
                        let _ = self.cmd_tx.send(res);
                    }
                    Action::Unwatch(path) => self.remove_watch(path),
                    Action::Stop => {
                        stopped = true;
                        for (_, ws) in &self.watches {
                            stop_watch(ws, &self.meta_tx);
                        }
                        break;
                    }
                }
            }

            if stopped {
                break;
            }

            unsafe {
                // wait with alertable flag so that the completion routine fires
                let waitres = kernel32::WaitForSingleObjectEx(self.wakeup_sem, 100, TRUE);
                if waitres == WAIT_OBJECT_0 {
                    let _ = self.meta_tx.send(MetaEvent::WatcherAwakened);
                }
            }
        }

        // we have to clean this up, since the watcher may be long gone
        unsafe {
            kernel32::CloseHandle(self.wakeup_sem);
        }
    }

    fn add_watch(&mut self, path: PathBuf, is_recursive: bool) -> Result<PathBuf> {
        // path must exist and be either a file or directory
        if !path.is_dir() && !path.is_file() {
            return Err(Error::Generic("Input watch path is neither a file nor a directory."
                .to_owned()));
        }

        let (watching_file, dir_target) = {
            if path.is_dir() {
                (false, path.clone())
            } else {
                // emulate file watching by watching the parent directory
                (true, path.parent().unwrap().to_path_buf())
            }
        };

        let encoded_path: Vec<u16> = dir_target.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle;
        unsafe {
            handle = kernel32::CreateFileW(encoded_path.as_ptr(),
                                           winnt::FILE_LIST_DIRECTORY,
                                           winnt::FILE_SHARE_READ | winnt::FILE_SHARE_DELETE |
                                           winnt::FILE_SHARE_WRITE,
                                           ptr::null_mut(),
                                           fileapi::OPEN_EXISTING,
                                           winbase::FILE_FLAG_BACKUP_SEMANTICS |
                                           winbase::FILE_FLAG_OVERLAPPED,
                                           ptr::null_mut());

            if handle == INVALID_HANDLE_VALUE {
                let err = if watching_file {
                    Err(Error::Generic("You attempted to watch a single file, but parent \
                                        directory could not be opened."
                        .to_owned()))
                } else {
                    // TODO: Call GetLastError for better error info?
                    Err(Error::PathNotFound)
                };
                return err;
            }
        }
        let wf = if watching_file {
            Some(path.clone())
        } else {
            None
        };
        // every watcher gets its own semaphore to signal completion
        let semaphore =
            unsafe { kernel32::CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
        if semaphore == ptr::null_mut() || semaphore == INVALID_HANDLE_VALUE {
            unsafe {
                kernel32::CloseHandle(handle);
            }
            return Err(Error::Generic("Failed to create semaphore for watch.".to_owned()));
        }
        let rd = ReadData {
            dir: dir_target,
            file: wf,
            complete_sem: semaphore,
            is_recursive: is_recursive,
        };
        let ws = WatchState {
            dir_handle: handle,
            complete_sem: semaphore,
        };
        self.watches.insert(path.clone(), ws);
        start_read(&rd, self.event_tx.clone(), handle);
        Ok(path.to_path_buf())
    }

    fn remove_watch(&mut self, path: PathBuf) {
        if let Some(ws) = self.watches.remove(&path) {
            stop_watch(&ws, &self.meta_tx);
        }
    }
}

fn stop_watch(ws: &WatchState, meta_tx: &Sender<MetaEvent>) {
    unsafe {
        let cio = kernel32::CancelIo(ws.dir_handle);
        let ch = kernel32::CloseHandle(ws.dir_handle);
        // have to wait for it, otherwise we leak the memory allocated for there read request
        if cio != 0 && ch != 0 {
            kernel32::WaitForSingleObjectEx(ws.complete_sem, INFINITE, TRUE);
        }
        kernel32::CloseHandle(ws.complete_sem);

    }
    let _ = meta_tx.send(MetaEvent::SingleWatchComplete);
}

fn start_read(rd: &ReadData, event_tx: Arc<Mutex<EventTx>>, handle: HANDLE) {
    let mut request = Box::new(ReadDirectoryRequest {
        event_tx: event_tx,
        handle: handle,
        buffer: [0u8; BUF_SIZE as usize],
        data: rd.clone(),
    });

    let flags =
        winnt::FILE_NOTIFY_CHANGE_FILE_NAME | winnt::FILE_NOTIFY_CHANGE_DIR_NAME |
        winnt::FILE_NOTIFY_CHANGE_ATTRIBUTES | winnt::FILE_NOTIFY_CHANGE_SIZE |
        winnt::FILE_NOTIFY_CHANGE_LAST_WRITE | winnt::FILE_NOTIFY_CHANGE_CREATION |
        winnt::FILE_NOTIFY_CHANGE_SECURITY;

    let monitor_subdir = if (&request.data.file).is_none() && request.data.is_recursive {
        1
    } else {
        0
    };

    unsafe {
        let mut overlapped: Box<OVERLAPPED> = Box::new(mem::zeroed());
        // When using callback based async requests, we are allowed to use the hEvent member
        // for our own purposes

        let req_buf = request.buffer.as_mut_ptr() as *mut c_void;
        let request_p = Box::into_raw(request) as *mut c_void;
        overlapped.hEvent = request_p;

        // This is using an asynchronous call with a completion routine for receiving notifications
        // An I/O completion port would probably be more performant
        let ret = kernel32::ReadDirectoryChangesW(handle,
                                                  req_buf,
                                                  BUF_SIZE,
                                                  monitor_subdir,
                                                  flags,
                                                  &mut 0u32 as *mut u32, // not used for async reqs
                                                  &mut *overlapped as *mut OVERLAPPED,
                                                  Some(handle_event));

        if ret == 0 {
            // error reading. retransmute request memory to allow drop.
            // allow overlapped to drop by omitting forget()
            let request: Box<ReadDirectoryRequest> = mem::transmute(request_p);

            kernel32::ReleaseSemaphore(request.data.complete_sem, 1, ptr::null_mut());
        } else {
            // read ok. forget overlapped to let the completion routine handle memory
            mem::forget(overlapped);
        }
    }
}

fn send_pending_rename_event(event: Option<RawEvent>, event_tx: &mut EventTx) {
    if let Some(e) = event {
        event_tx.send(RawEvent {
            path: e.path,
            op: Ok(op::REMOVE),
            cookie: None,
        });
    }
}

unsafe extern "system" fn handle_event(error_code: u32,
                                       _bytes_written: u32,
                                       overlapped: LPOVERLAPPED) {
    let overlapped: Box<OVERLAPPED> = Box::from_raw(overlapped);
    let request: Box<ReadDirectoryRequest> = Box::from_raw(overlapped.hEvent as *mut _);

    if error_code == ERROR_OPERATION_ABORTED {
        // received when dir is unwatched or watcher is shutdown; return and let overlapped/request
        // get drop-cleaned
        kernel32::ReleaseSemaphore(request.data.complete_sem, 1, ptr::null_mut());
        return;
    }

    // Get the next request queued up as soon as possible
    start_read(&request.data, request.event_tx.clone(), request.handle);

    let event_tx_lock = request.event_tx.lock();
    if let Ok(mut event_tx) = event_tx_lock {
        let mut rename_event = None;

        // The FILE_NOTIFY_INFORMATION struct has a variable length due to the variable length
        // string as its last member.  Each struct contains an offset for getting the next entry in
        // the buffer.
        let mut cur_offset: *const u8 = request.buffer.as_ptr();
        let mut cur_entry: *const FILE_NOTIFY_INFORMATION = mem::transmute(cur_offset);
        loop {
            // filename length is size in bytes, so / 2
            let len = (*cur_entry).FileNameLength as usize / 2;
            let encoded_path: &[u16] = slice::from_raw_parts((*cur_entry).FileName.as_ptr(), len);
            // prepend root to get a full path
            let path = request.data.dir.join(PathBuf::from(OsString::from_wide(encoded_path)));

            // if we are watching a single file, ignore the event unless the path is exactly
            // the watched file
            let skip = match request.data.file {
                None => false,
                Some(ref watch_path) => *watch_path != path,
            };

            if !skip {
                if (*cur_entry).Action == winnt::FILE_ACTION_RENAMED_OLD_NAME {
                    send_pending_rename_event(rename_event, &mut event_tx);
                    if request.data.file.is_some() {
                        event_tx.send(RawEvent {
                            path: Some(path),
                            op: Ok(op::RENAME),
                            cookie: None,
                        });
                        rename_event = None;
                    } else {
                        COOKIE_COUNTER = COOKIE_COUNTER.wrapping_add(1);
                        rename_event = Some(RawEvent {
                            path: Some(path),
                            op: Ok(op::RENAME),
                            cookie: Some(COOKIE_COUNTER),
                        });
                    }
                } else {
                    let mut o = Op::empty();
                    let mut c = None;

                    match (*cur_entry).Action {
                        winnt::FILE_ACTION_RENAMED_NEW_NAME => {
                            if let Some(e) = rename_event {
                                if let Some(cookie) = e.cookie {
                                    event_tx.send(e);
                                    o.insert(op::RENAME);
                                    c = Some(cookie);
                                } else {
                                    o.insert(op::CREATE);
                                }
                            } else {
                                o.insert(op::CREATE);
                            }
                            rename_event = None;
                        }
                        winnt::FILE_ACTION_ADDED => o.insert(op::CREATE),
                        winnt::FILE_ACTION_REMOVED => o.insert(op::REMOVE),
                        winnt::FILE_ACTION_MODIFIED => o.insert(op::WRITE),
                        _ => (),
                    };

                    send_pending_rename_event(rename_event, &mut event_tx);
                    rename_event = None;

                    event_tx.send(RawEvent {
                        path: Some(path),
                        op: Ok(o),
                        cookie: c,
                    });
                }
            }

            if (*cur_entry).NextEntryOffset == 0 {
                break;
            }
            cur_offset = cur_offset.offset((*cur_entry).NextEntryOffset as isize);
            cur_entry = mem::transmute(cur_offset);
        }

        send_pending_rename_event(rename_event, &mut event_tx);
    }
}

pub struct ReadDirectoryChangesWatcher {
    tx: Sender<Action>,
    cmd_rx: Receiver<Result<PathBuf>>,
    wakeup_sem: HANDLE,
}

impl ReadDirectoryChangesWatcher {
    pub fn create(tx: Sender<RawEvent>,
                  meta_tx: Sender<MetaEvent>)
                  -> Result<ReadDirectoryChangesWatcher> {
        let (cmd_tx, cmd_rx) = channel();

        let wakeup_sem =
            unsafe { kernel32::CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
        if wakeup_sem == ptr::null_mut() || wakeup_sem == INVALID_HANDLE_VALUE {
            return Err(Error::Generic("Failed to create wakeup semaphore.".to_owned()));
        }

        let event_tx = EventTx::Raw { tx: tx };

        let action_tx = ReadDirectoryChangesServer::start(event_tx, meta_tx, cmd_tx, wakeup_sem);

        Ok(ReadDirectoryChangesWatcher {
            tx: action_tx,
            cmd_rx: cmd_rx,
            wakeup_sem: wakeup_sem,
        })
    }

    pub fn create_debounced(tx: Sender<DebouncedEvent>,
                            meta_tx: Sender<MetaEvent>,
                            delay: Duration)
                            -> Result<ReadDirectoryChangesWatcher> {
        let (cmd_tx, cmd_rx) = channel();

        let wakeup_sem =
            unsafe { kernel32::CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
        if wakeup_sem == ptr::null_mut() || wakeup_sem == INVALID_HANDLE_VALUE {
            return Err(Error::Generic("Failed to create wakeup semaphore.".to_owned()));
        }

        let event_tx = EventTx::Debounced {
            tx: tx.clone(),
            debounce: Debounce::new(delay, tx),
        };

        let action_tx = ReadDirectoryChangesServer::start(event_tx, meta_tx, cmd_tx, wakeup_sem);

        Ok(ReadDirectoryChangesWatcher {
            tx: action_tx,
            cmd_rx: cmd_rx,
            wakeup_sem: wakeup_sem,
        })
    }

    fn wakeup_server(&mut self) {
        // breaks the server out of its wait state.  right now this is really just an optimization,
        // so that if you add a watch you don't block for 100ms in watch() while the
        // server sleeps.
        unsafe {
            kernel32::ReleaseSemaphore(self.wakeup_sem, 1, ptr::null_mut());
        }
    }

    fn send_action_require_ack(&mut self, action: Action, pb: &PathBuf) -> Result<()> {
        match self.tx.send(action) {
            Err(_) => Err(Error::Generic("Error sending to internal channel".to_owned())),
            Ok(_) => {
                // wake 'em up, we don't want to wait around for the ack
                self.wakeup_server();

                match self.cmd_rx.recv() {
                    Err(_) => {
                        Err(Error::Generic("Error receiving from command channel".to_owned()))
                    }
                    Ok(ack_res) => {
                        match ack_res {
                            Err(e) => Err(Error::Generic(format!("Error in watcher: {:?}", e))),
                            Ok(ack_pb) => {
                                if pb.as_path() != ack_pb.as_path() {
                                    Err(Error::Generic(format!("Expected ack for {:?} but got \
                                                                ack for {:?}",
                                                               pb,
                                                               ack_pb)))
                                } else {
                                    Ok(())
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Watcher for ReadDirectoryChangesWatcher {
    fn new_raw(tx: Sender<RawEvent>) -> Result<ReadDirectoryChangesWatcher> {
        // create dummy channel for meta event
        let (meta_tx, _) = channel();
        ReadDirectoryChangesWatcher::create(tx, meta_tx)
    }

    fn new(tx: Sender<DebouncedEvent>, delay: Duration) -> Result<ReadDirectoryChangesWatcher> {
        // create dummy channel for meta event
        let (meta_tx, _) = channel();
        ReadDirectoryChangesWatcher::create_debounced(tx, meta_tx, delay)
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        let pb = if path.as_ref().is_absolute() {
            path.as_ref().to_owned()
        } else {
            let p = try!(env::current_dir().map_err(Error::Io));
            p.join(path)
        };
        // path must exist and be either a file or directory
        if !pb.is_dir() && !pb.is_file() {
            return Err(Error::Generic("Input watch path is neither a file nor a directory."
                .to_owned()));
        }
        self.send_action_require_ack(Action::Watch(pb.clone(), recursive_mode), &pb)
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let pb = if path.as_ref().is_absolute() {
            path.as_ref().to_owned()
        } else {
            let p = try!(env::current_dir().map_err(Error::Io));
            p.join(path)
        };
        let res = self.tx
            .send(Action::Unwatch(pb))
            .map_err(|_| Error::Generic("Error sending to internal channel".to_owned()));
        self.wakeup_server();
        res
    }
}

impl Drop for ReadDirectoryChangesWatcher {
    fn drop(&mut self) {
        let _ = self.tx.send(Action::Stop);
        // better wake it up
        self.wakeup_server();
    }
}

// `ReadDirectoryChangesWatcher` is not Send/Sync because of the semaphore Handle.
// As said elsewhere it's perfectly safe to send it accross threads.
unsafe impl Send for ReadDirectoryChangesWatcher {}
// Because all public methods are `&mut self` it's also perfectly safe to share references.
unsafe impl Sync for ReadDirectoryChangesWatcher {}
