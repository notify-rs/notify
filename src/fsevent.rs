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
//! TODO: document event translation
//!
//! [ref]: https://developer.apple.com/library/mac/documentation/Darwin/Reference/FSEvents_Ref/

#![allow(non_upper_case_globals, dead_code)]

use crate::event::*;
use crate::{Config, Error, EventFn, RecursiveMode, Result, Watcher};
use crossbeam_channel::{unbounded, Receiver, Sender};
use fsevent as fse;
use fsevent_sys as fs;
use fsevent_sys::core_foundation as cf;
use libc;
use std::collections::HashMap;
use std::convert::AsRef;
use std::ffi::CStr;
use std::mem::transmute;
use std::os::raw;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::str::from_utf8;
use std::sync::Arc;
use std::thread;

/// FSEvents-based `Watcher` implementation
pub struct FsEventWatcher {
    paths: cf::CFMutableArrayRef,
    since_when: fs::FSEventStreamEventId,
    latency: cf::CFTimeInterval,
    flags: fs::FSEventStreamCreateFlags,
    event_fn: Arc<dyn EventFn>,
    runloop: Option<usize>,
    context: Option<Box<StreamContextInfo>>,
    recursive_info: HashMap<PathBuf, bool>,
}

// CFMutableArrayRef is a type alias to *mut libc::c_void, so FsEventWatcher is not Send/Sync
// automatically. It's Send because the pointer is not used in other threads.
unsafe impl Send for FsEventWatcher {}

// It's Sync because all methods that change the mutable state use `&mut self`.
unsafe impl Sync for FsEventWatcher {}

fn translate_flags(flags: fse::StreamFlags, precise: bool) -> Vec<Event> {
    let mut evs = Vec::new();

    // «Denotes a sentinel event sent to mark the end of the "historical" events
    // sent as a result of specifying a `sinceWhen` value in the FSEvents.Create
    // call that created this event stream. After invoking the client's callback
    // with all the "historical" events that occurred before now, the client's
    // callback will be invoked with an event where the HistoryDone flag is set.
    // The client should ignore the path supplied in this callback.»
    // — https://www.mbsplugins.eu/FSEventsNextEvent.shtml
    //
    // As a result, we just stop processing here and return an empty vec, which
    // will ignore this completely and not emit any Events whatsoever.
    if flags.contains(fse::StreamFlags::HISTORY_DONE) {
        return evs;
    }

    // FSEvents provides two possible hints as to why events were dropped,
    // however documentation on what those mean is scant, so we just pass them
    // through in the info attr field. The intent is clear enough, and the
    // additional information is provided if the user wants it.
    if flags.contains(fse::StreamFlags::MUST_SCAN_SUBDIRS) {
        let e = Event::new(EventKind::Other).set_flag(Flag::Rescan);
        evs.push(if flags.contains(fse::StreamFlags::USER_DROPPED) {
            e.set_info("rescan: user dropped")
        } else if flags.contains(fse::StreamFlags::KERNEL_DROPPED) {
            e.set_info("rescan: kernel dropped")
        } else {
            e
        });
    }

    // In imprecise mode, let's not even bother parsing the kind of the event
    // except for the above very special events.
    if !precise {
        evs.push(Event::new(EventKind::Any));
        return evs;
    }

    // This is most likely a rename or a removal. We assume rename but may want
    // to figure out if it was a removal some way later (TODO). To denote the
    // special nature of the event, we add an info string.
    if flags.contains(fse::StreamFlags::ROOT_CHANGED) {
        evs.push(
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
                .set_info("root changed"),
        );
    }

    // A path was mounted at the event path; we treat that as a create.
    if flags.contains(fse::StreamFlags::MOUNT) {
        evs.push(Event::new(EventKind::Create(CreateKind::Other)).set_info("mount"));
    }

    // A path was unmounted at the event path; we treat that as a remove.
    if flags.contains(fse::StreamFlags::UNMOUNT) {
        evs.push(Event::new(EventKind::Remove(RemoveKind::Other)).set_info("mount"));
    }

    if flags.contains(fse::StreamFlags::ITEM_CREATED) {
        evs.push(if flags.contains(fse::StreamFlags::IS_DIR) {
            Event::new(EventKind::Create(CreateKind::Folder))
        } else if flags.contains(fse::StreamFlags::IS_FILE) {
            Event::new(EventKind::Create(CreateKind::File))
        } else {
            let e = Event::new(EventKind::Create(CreateKind::Other));
            if flags.contains(fse::StreamFlags::IS_SYMLINK) {
                e.set_info("is: symlink")
            } else if flags.contains(fse::StreamFlags::IS_HARDLINK) {
                e.set_info("is: hardlink")
            } else if flags.contains(fse::StreamFlags::ITEM_CLONED) {
                e.set_info("is: clone")
            } else {
                Event::new(EventKind::Create(CreateKind::Any))
            }
        });
    }

    if flags.contains(fse::StreamFlags::ITEM_REMOVED) {
        evs.push(if flags.contains(fse::StreamFlags::IS_DIR) {
            Event::new(EventKind::Remove(RemoveKind::Folder))
        } else if flags.contains(fse::StreamFlags::IS_FILE) {
            Event::new(EventKind::Remove(RemoveKind::File))
        } else {
            let e = Event::new(EventKind::Remove(RemoveKind::Other));
            if flags.contains(fse::StreamFlags::IS_SYMLINK) {
                e.set_info("is: symlink")
            } else if flags.contains(fse::StreamFlags::IS_HARDLINK) {
                e.set_info("is: hardlink")
            } else if flags.contains(fse::StreamFlags::ITEM_CLONED) {
                e.set_info("is: clone")
            } else {
                Event::new(EventKind::Remove(RemoveKind::Any))
            }
        });
    }

    if flags.contains(fse::StreamFlags::ITEM_RENAMED) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::From,
        ))));
    }

    // This is only described as "metadata changed", but it may be that it's
    // only emitted for some more precise subset of events... if so, will need
    // amending, but for now we have an Any-shaped bucket to put it in.
    if flags.contains(fse::StreamFlags::INODE_META_MOD) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Any,
        ))));
    }

    if flags.contains(fse::StreamFlags::FINDER_INFO_MOD) {
        evs.push(
            Event::new(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Other)))
                .set_info("meta: finder info"),
        );
    }

    if flags.contains(fse::StreamFlags::ITEM_CHANGE_OWNER) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Ownership,
        ))));
    }

    if flags.contains(fse::StreamFlags::ITEM_XATTR_MOD) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Extended,
        ))));
    }

    // This is specifically described as a data change, which we take to mean
    // is a content change.
    if flags.contains(fse::StreamFlags::ITEM_MODIFIED) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Data(
            DataChange::Content,
        ))));
    }

    if flags.contains(fse::StreamFlags::OWN_EVENT) {
        for ev in &mut evs {
            ev.attrs.insert(ProcessID(std::process::id()));
        }
    }

    evs
}

struct StreamContextInfo {
    event_fn: Arc<dyn EventFn>,
    done: Receiver<()>,
    recursive_info: HashMap<PathBuf, bool>,
}

extern "C" {
    pub fn CFRunLoopIsWaiting(runloop: cf::CFRunLoopRef) -> bool;
}

impl FsEventWatcher {
    fn from_event_fn(event_fn: Arc<dyn EventFn>) -> Result<Self> {
        Ok(FsEventWatcher {
            paths: unsafe {
                cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 0, &cf::kCFTypeArrayCallBacks)
            },
            since_when: fs::kFSEventStreamEventIdSinceNow,
            latency: 0.0,
            flags: fs::kFSEventStreamCreateFlagFileEvents | fs::kFSEventStreamCreateFlagNoDefer,
            event_fn: event_fn,
            runloop: None,
            context: None,
            recursive_info: HashMap::new(),
        })
    }

    fn watch_inner(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        self.stop();
        let result = self.append_path(path, recursive_mode);
        // ignore return error: may be empty path list
        let _ = self.run();
        result
    }

    fn unwatch_inner(&mut self, path: &Path) -> Result<()> {
        self.stop();
        let result = self.remove_path(path);
        // ignore return error: may be empty path list
        let _ = self.run();
        result
    }

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
                let runloop = runloop as *mut raw::c_void;

                while !CFRunLoopIsWaiting(runloop) {
                    thread::yield_now();
                }

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
            let mut err: cf::CFErrorRef = ptr::null_mut();
            let cf_path = cf::str_path_to_cfstring_ref(str_path, &mut err);

            let mut to_remove = Vec::new();
            for idx in 0..cf::CFArrayGetCount(self.paths) {
                let item = cf::CFArrayGetValueAtIndex(self.paths, idx);
                if cf::CFStringCompare(item, cf_path, cf::kCFCompareCaseInsensitive)
                    == cf::kCFCompareEqualTo
                {
                    to_remove.push(idx);
                }
            }

            for idx in to_remove.iter().rev() {
                cf::CFArrayRemoveValueAtIndex(self.paths, *idx);
            }
        }
        let p = if let Ok(canonicalized_path) = path.as_ref().canonicalize() {
            canonicalized_path
        } else {
            path.as_ref().to_owned()
        };
        match self.recursive_info.remove(&p) {
            Some(_) => Ok(()),
            None => Err(Error::watch_not_found()),
        }
    }

    // https://github.com/thibaudgg/rb-fsevent/blob/master/ext/fsevent_watch/main.c
    fn append_path<P: AsRef<Path>>(
        &mut self,
        path: P,
        recursive_mode: RecursiveMode,
    ) -> Result<()> {
        if !path.as_ref().exists() {
            return Err(Error::path_not_found().add_path(path.as_ref().into()));
        }
        let str_path = path.as_ref().to_str().unwrap();
        unsafe {
            let mut err: cf::CFErrorRef = ptr::null_mut();
            let cf_path = cf::str_path_to_cfstring_ref(str_path, &mut err);
            cf::CFArrayAppendValue(self.paths, cf_path);
            cf::CFRelease(cf_path);
        }
        self.recursive_info.insert(
            path.as_ref().to_path_buf().canonicalize().unwrap(),
            recursive_mode.is_recursive(),
        );
        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        if unsafe { cf::CFArrayGetCount(self.paths) } == 0 {
            // TODO: Reconstruct and add paths to error
            return Err(Error::path_not_found());
        }

        // done channel is used to sync quit status of runloop thread
        let (done_tx, done_rx) = unbounded();

        let info = StreamContextInfo {
            event_fn: self.event_fn.clone(),
            done: done_rx,
            recursive_info: self.recursive_info.clone(),
        };

        self.context = Some(Box::new(info));

        let stream_context = fs::FSEventStreamContext {
            version: 0,
            info: unsafe { transmute(self.context.as_ref().map(|ctx| &**ctx)) },
            retain: None,
            release: None,
            copy_description: None,
        };

        let stream = unsafe {
            fs::FSEventStreamCreate(
                cf::kCFAllocatorDefault,
                callback,
                &stream_context,
                self.paths,
                self.since_when,
                self.latency,
                self.flags,
            )
        };

        // move into thread
        let dummy = stream as usize;
        // channel to pass runloop around
        let (rl_tx, rl_rx) = unbounded();

        thread::spawn(move || {
            let stream = dummy as *mut raw::c_void;
            unsafe {
                let cur_runloop = cf::CFRunLoopGetCurrent();

                fs::FSEventStreamScheduleWithRunLoop(
                    stream,
                    cur_runloop,
                    cf::kCFRunLoopDefaultMode,
                );
                fs::FSEventStreamStart(stream);

                // the calling to CFRunLoopRun will be terminated by CFRunLoopStop call in drop()
                rl_tx
                    .send(cur_runloop as *mut libc::c_void as usize)
                    .expect("Unable to send runloop to watcher");
                cf::CFRunLoopRun();
                fs::FSEventStreamStop(stream);
                fs::FSEventStreamInvalidate(stream);
                fs::FSEventStreamRelease(stream);
            }
            done_tx
                .send(())
                .expect("error while signal run loop is done");
        });
        // block until runloop has been set
        self.runloop = Some(rl_rx.recv().unwrap());

        Ok(())
    }

    fn configure_raw_mode(&mut self, _config: Config, tx: Sender<Result<bool>>) {
        tx.send(Ok(false))
            .expect("configuration channel disconnect");
    }
}

#[allow(unused_variables)]
#[doc(hidden)]
pub extern "C" fn callback(
    stream_ref: fs::FSEventStreamRef,
    info: *mut libc::c_void,
    num_events: libc::size_t,         // size_t numEvents
    event_paths: *mut libc::c_void,   // void *eventPaths
    event_flags: *const libc::c_void, // const FSEventStreamEventFlags eventFlags[]
    event_ids: *const libc::c_void,   // const FSEventStreamEventId eventIds[]
) {
    unsafe {
        callback_impl(
            stream_ref,
            info,
            num_events,
            event_paths,
            event_flags,
            event_ids,
        )
    }
}

#[allow(unused_variables)]
#[doc(hidden)]
unsafe fn callback_impl(
    stream_ref: fs::FSEventStreamRef,
    info: *mut libc::c_void,
    num_events: libc::size_t,         // size_t numEvents
    event_paths: *mut libc::c_void,   // void *eventPaths
    event_flags: *const libc::c_void, // const FSEventStreamEventFlags eventFlags[]
    event_ids: *const libc::c_void,   // const FSEventStreamEventId eventIds[]
) {
    let num = num_events as usize;
    let e_ptr = event_flags as *mut u32;
    let i_ptr = event_ids as *mut u64;
    let info = transmute::<_, *const StreamContextInfo>(info);

    let paths: &[*const libc::c_char] = transmute(slice::from_raw_parts(event_paths, num));
    let flags = slice::from_raw_parts_mut(e_ptr, num);
    let ids = slice::from_raw_parts_mut(i_ptr, num);

    let event_fn = &(*info).event_fn;

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

        if !handle_event {
            continue;
        }

        for ev in translate_flags(flag, true).into_iter() {
            // TODO: precise
            let ev = ev.add_path(path.clone());
            (*event_fn)(Ok(ev));
        }
    }
}

impl Watcher for FsEventWatcher {
    fn new_immediate<F: EventFn>(event_fn: F) -> Result<FsEventWatcher> {
        FsEventWatcher::from_event_fn(Arc::new(event_fn))
    }

    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Result<()> {
        self.watch_inner(path.as_ref(), recursive_mode)
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        self.unwatch_inner(path.as_ref())
    }

    fn configure(&mut self, config: Config) -> Result<bool> {
        let (tx, rx) = unbounded();
        self.configure_raw_mode(config, tx);
        rx.recv()?
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

    let (tx, rx) = std::sync::mpsc::channel();
    let event_fn = move |res| tx.send(res).unwrap();

    {
        let mut watcher: RecommendedWatcher = Watcher::new_immediate(event_fn).unwrap();
        watcher.watch("../../", RecursiveMode::Recursive).unwrap();
        thread::sleep(Duration::from_millis(2000));
        println!("is running -> {}", watcher.is_running());

        thread::sleep(Duration::from_millis(1000));
        watcher.unwatch("../..").unwrap();
        println!("is running -> {}", watcher.is_running());
    }

    thread::sleep(Duration::from_millis(1000));

    for res in rx {
        let e = res.unwrap();
        println!("debug => {:?} {:?}", e.kind, e.paths);
    }

    println!("in test: {} works", file!());
}
