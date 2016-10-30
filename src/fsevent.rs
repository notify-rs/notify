//! Watcher implementation for Darwin's FSEvents API
//!
//! The FSEvents API provides a mechanism to notify clients about directories they ought to re-scan
//! in order to keep their internal data structures up-to-date with respect to the true state of
//! the file system. (For example, when files or directories are created, modified, or removed.) It
//! sends these notifications "in bulk", possibly notifying the client of changes to several
//! directories in a single callback.
//!
//! For more information see the [FSEvents API reference][ref].
//!
//! [ref]: https://developer.apple.com/library/mac/documentation/Darwin/Reference/FSEvents_Ref/

#![allow(non_upper_case_globals, dead_code)]
extern crate fsevent as fse;

use fsevent_sys::core_foundation as cf;
use fsevent_sys::fsevent as fs;
use libc;
use std::collections::HashMap;
use std::convert::AsRef;
use std::ffi::CStr;
use std::mem::transmute;
use std::path::{Path, PathBuf};
use std::slice;
use std::str::from_utf8;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::time::Duration;
use super::{Error, RawEvent, DebouncedEvent, op, Result, Watcher, RecursiveMode};
use super::debounce::{Debounce, EventTx};

/// FSEvents-based `Watcher` implementation
pub struct FsEventWatcher {
    paths: cf::CFMutableArrayRef,
    since_when: fs::FSEventStreamEventId,
    latency: cf::CFTimeInterval,
    flags: fs::FSEventStreamCreateFlags,
    event_tx: Arc<Mutex<EventTx>>,
    runloop: Option<usize>,
    context: Option<Box<StreamContextInfo>>,
    recursive_info: HashMap<PathBuf, bool>,
}

// CFMutableArrayRef is a type alias to *mut libc::c_void, so FsEventWatcher is not Send/Sync
// automatically. It's Send because the pointer is not used in other threads.
unsafe impl Send for FsEventWatcher {}

// It's Sync because all methods that change the mutable state use `&mut self`.
unsafe impl Sync for FsEventWatcher {}

fn translate_flags(flags: fse::StreamFlags) -> op::Op {
    let mut ret = op::Op::empty();
    if flags.contains(fse::ITEM_XATTR_MOD) || flags.contains(fse::ITEM_CHANGE_OWNER) {
        ret.insert(op::CHMOD);
    }
    if flags.contains(fse::ITEM_CREATED) {
        ret.insert(op::CREATE);
    }
    if flags.contains(fse::ITEM_REMOVED) {
        ret.insert(op::REMOVE);
    }
    if flags.contains(fse::ITEM_RENAMED) {
        ret.insert(op::RENAME);
    }
    if flags.contains(fse::ITEM_MODIFIED) {
        ret.insert(op::WRITE);
    }
    ret
}

fn send_pending_rename_event(event: Option<RawEvent>, event_tx: &mut EventTx) {
    if let Some(e) = event {
        event_tx.send(RawEvent {
            path: e.path,
            op: e.op,
            cookie: None,
        });
    }
}

struct StreamContextInfo {
    event_tx: Arc<Mutex<EventTx>>,
    done: Receiver<()>,
    recursive_info: HashMap<PathBuf, bool>,
}

impl FsEventWatcher {
    #[inline]
    fn is_running(&self) -> bool {
        self.runloop.is_some()
    }

    fn stop(&mut self) {
        if !self.is_running() {
            return;
        }

        if let Some(runloop) = self.runloop {
            unsafe {
                let runloop = runloop as *mut libc::c_void;
                cf::CFRunLoopStop(runloop);
            }
        }

        self.runloop = None;
        if let Some(ref context_info) = self.context {
            // sync done channel
            match context_info.done.recv() {
                Ok(()) => (),
                Err(_) => panic!("the runloop may not be finished!"),
            }
        }

        self.context = None;
    }

    fn remove_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let str_path = path.as_ref().to_str().unwrap();
        unsafe {
            let cf_path = cf::str_path_to_cfstring_ref(str_path);

            for idx in 0..cf::CFArrayGetCount(self.paths) {
                let item = cf::CFArrayGetValueAtIndex(self.paths, idx);
                if cf::CFStringCompare(item, cf_path, cf::kCFCompareCaseInsensitive) ==
                   cf::kCFCompareEqualTo {
                    cf::CFArrayRemoveValueAtIndex(self.paths, idx);
                }
            }
        }
        let p = if let Ok(canonicalized_path) = path.as_ref().canonicalize() {
            canonicalized_path
        } else {
            path.as_ref().to_owned()
        };
        match self.recursive_info.remove(&p) {
            Some(_) => Ok(()),
            None => Err(Error::WatchNotFound),
        }
    }

    // https://github.com/thibaudgg/rb-fsevent/blob/master/ext/fsevent_watch/main.c
    fn append_path<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) {
        let str_path = path.as_ref().to_str().unwrap();
        unsafe {
            let cf_path = cf::str_path_to_cfstring_ref(str_path);
            cf::CFArrayAppendValue(self.paths, cf_path);
            cf::CFRelease(cf_path);
        }
        self.recursive_info.insert(path.as_ref().to_path_buf().canonicalize().unwrap(),
                                   recursive_mode.is_recursive());
    }

    fn run(&mut self) -> Result<()> {
        if unsafe { cf::CFArrayGetCount(self.paths) } == 0 {
            return Err(Error::PathNotFound);
        }

        // done channel is used to sync quit status of runloop thread
        let (done_tx, done_rx) = channel();

        let info = StreamContextInfo {
            event_tx: self.event_tx.clone(),
            done: done_rx,
            recursive_info: self.recursive_info.clone(),
        };

        self.context = Some(Box::new(info));

        let stream_context = fs::FSEventStreamContext {
            version: 0,
            info: unsafe { transmute(self.context.as_ref().map(|ctx| &**ctx)) },
            retain: cf::NULL,
            copy_description: cf::NULL,
        };

        let cb = callback as *mut _;
        let stream = unsafe {
            fs::FSEventStreamCreate(cf::kCFAllocatorDefault,
                                    cb,
                                    &stream_context,
                                    self.paths,
                                    self.since_when,
                                    self.latency,
                                    self.flags)
        };

        // move into thread
        let dummy = stream as usize;
        // channel to pass runloop around
        let (rl_tx, rl_rx) = channel();

        thread::spawn(move || {
            let stream = dummy as *mut libc::c_void;
            unsafe {
                let cur_runloop = cf::CFRunLoopGetCurrent();

                fs::FSEventStreamScheduleWithRunLoop(stream,
                                                     cur_runloop,
                                                     cf::kCFRunLoopDefaultMode);
                fs::FSEventStreamStart(stream);

                // the calling to CFRunLoopRun will be terminated by CFRunLoopStop call in drop()
                rl_tx.send(cur_runloop as *mut libc::c_void as usize)
                    .expect("Unable to send runloop to watcher");
                cf::CFRunLoopRun();
                fs::FSEventStreamStop(stream);
                fs::FSEventStreamInvalidate(stream);
                fs::FSEventStreamRelease(stream);
            }
            done_tx.send(()).expect("error while signal run loop is done");
        });
        // block until runloop has been set
        self.runloop = Some(rl_rx.recv().unwrap());

        Ok(())
    }
}

#[allow(unused_variables)]
#[doc(hidden)]
pub unsafe extern "C" fn callback(
  stream_ref: fs::FSEventStreamRef,
  info: *mut libc::c_void,
  num_events: libc::size_t,                // size_t numEvents
  event_paths: *const *const libc::c_char, // void *eventPaths
  event_flags: *mut libc::c_void,          // const FSEventStreamEventFlags eventFlags[]
  event_ids: *mut libc::c_void,            // const FSEventStreamEventId eventIds[]
  ) {
    let num = num_events as usize;
    let e_ptr = event_flags as *mut u32;
    let i_ptr = event_ids as *mut u64;
    let info = transmute::<_, *const StreamContextInfo>(info);

    let paths: &[*const libc::c_char] = transmute(slice::from_raw_parts(event_paths, num));
    let flags = slice::from_raw_parts_mut(e_ptr, num);
    let ids = slice::from_raw_parts_mut(i_ptr, num);

    if let Ok(mut event_tx) = (*info).event_tx.lock() {
        let mut rename_event: Option<RawEvent> = None;

        for p in 0..num {
            let i = CStr::from_ptr(paths[p]).to_bytes();
            let flag = fse::StreamFlags::from_bits(flags[p] as u32)
                .expect(format!("Unable to decode StreamFlags: {}", flags[p] as u32).as_ref());
            let id = ids[p];

            let path = PathBuf::from(from_utf8(i).expect("Invalid UTF8 string."));

            let mut handle_event = false;
            for (p, r) in &(*info).recursive_info {
                if path.starts_with(p) {
                    if *r || &path == p {
                        handle_event = true;
                        break;
                    } else if let Some(parent_path) = path.parent() {
                        if parent_path == p {
                            handle_event = true;
                            break;
                        }
                    }
                }
            }

            if flag.contains(fse::MUST_SCAN_SUBDIRS) {
                event_tx.send(RawEvent {
                    path: None,
                    op: Ok(op::RESCAN),
                    cookie: None,
                });
            }

            if handle_event {
                if flag.contains(fse::ITEM_RENAMED) {
                    if let Some(e) = rename_event {
                        if e.cookie == Some((id - 1) as u32) {
                            event_tx.send(e);
                            event_tx.send(RawEvent {
                                op: Ok(translate_flags(flag)),
                                path: Some(path),
                                cookie: Some((id - 1) as u32),
                            });
                            rename_event = None;
                        } else {
                            send_pending_rename_event(Some(e), &mut event_tx);
                            rename_event = Some(RawEvent {
                                path: Some(path),
                                op: Ok(translate_flags(flag)),
                                cookie: Some(id as u32),
                            });
                        }
                    } else {
                        rename_event = Some(RawEvent {
                            path: Some(path),
                            op: Ok(translate_flags(flag)),
                            cookie: Some(id as u32),
                        });
                    }
                } else {
                    send_pending_rename_event(rename_event, &mut event_tx);
                    rename_event = None;

                    event_tx.send(RawEvent {
                        op: Ok(translate_flags(flag)),
                        path: Some(path),
                        cookie: None,
                    });
                }
            }
        }

        send_pending_rename_event(rename_event, &mut event_tx);
    }
}


impl Watcher for FsEventWatcher {
    fn new_raw(tx: Sender<RawEvent>) -> Result<FsEventWatcher> {
        Ok(FsEventWatcher {
            paths: unsafe {
                cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 0, &cf::kCFTypeArrayCallBacks)
            },
            since_when: fs::kFSEventStreamEventIdSinceNow,
            latency: 0.0,
            flags: fs::kFSEventStreamCreateFlagFileEvents | fs::kFSEventStreamCreateFlagNoDefer,
            event_tx: Arc::new(Mutex::new(EventTx::Raw { tx: tx })),
            runloop: None,
            context: None,
            recursive_info: HashMap::new(),
        })
    }

    fn new(tx: Sender<DebouncedEvent>, delay: Duration) -> Result<FsEventWatcher> {
        Ok(FsEventWatcher {
            paths: unsafe {
                cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 0, &cf::kCFTypeArrayCallBacks)
            },
            since_when: fs::kFSEventStreamEventIdSinceNow,
            latency: 0.0,
            flags: fs::kFSEventStreamCreateFlagFileEvents | fs::kFSEventStreamCreateFlagNoDefer,
            event_tx: Arc::new(Mutex::new(EventTx::Debounced {
                tx: tx.clone(),
                debounce: Debounce::new(delay, tx),
            })),
            runloop: None,
            context: None,
            recursive_info: HashMap::new(),
        })
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        self.stop();
        self.append_path(path, recursive_mode);
        self.run()
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        self.stop();
        let result = self.remove_path(path);
        // ignore return error: may be empty path list
        let _ = self.run();
        result
    }
}

impl Drop for FsEventWatcher {
    fn drop(&mut self) {
        self.stop();
        unsafe {
            cf::CFRelease(self.paths);
        }
    }
}

#[test]
fn test_fsevent_watcher_drop() {
    use super::*;
    use std::time::Duration;

    let (tx, rx) = channel();

    {
        let mut watcher: RecommendedWatcher = Watcher::new_raw(tx).unwrap();
        watcher.watch("../../", RecursiveMode::Recursive).unwrap();
        thread::sleep(Duration::from_millis(2000));
        println!("is running -> {}", watcher.is_running());

        thread::sleep(Duration::from_millis(1000));
        watcher.unwatch("../..").unwrap();
        println!("is running -> {}", watcher.is_running());
    }

    thread::sleep(Duration::from_millis(1000));

    // if drop() works, this loop will quit after all Sender freed
    // otherwise will block forever
    for e in rx.iter() {
        println!("debug => {:?} {:?}",
                 e.op.map(|e| e.bits()).unwrap_or(0),
                 e.path);
    }

    println!("in test: {} works", file!());
}
