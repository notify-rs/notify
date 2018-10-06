use libc;
use std::collections::HashMap;
use std::path::{PathBuf, Path};
use std::sync::{atomic, Arc, Barrier};
use std::{slice, mem};
use std::time::{Instant, Duration};
use std::ffi::{CStr, OsString};
use std::os::unix::ffi::OsStringExt;
use std::ptr;
use std::thread;

use backend::event::*;
use super::WaitQueue;

use fsevent_rs as fse;
use fsevent_sys::core_foundation as cf;
use fsevent_sys::fsevent as fs;

/// A type for managing an FSEventStream.
pub struct FsEventWatcher {
    runloop_ptr: Arc<atomic::AtomicPtr<libc::c_void>>,
}

/// A previously received FSEvent.
///
/// The FSEvent API aggregates events, clearing an internal cache every
/// 30s or so. We keep track of previously received events, to determine
/// of this might be going on.
#[derive(Debug, Clone, Copy)]
struct HistoricalEvent {
    timestamp: Instant,
    flags: fse::StreamFlags,
}

/// A context contains references needed when handling the FSEvent callback.
struct Context {
    queue: WaitQueue,
    history: HashMap<PathBuf, HistoricalEvent>,
}

/// An iterator over FSEvent events received from a callback.
struct EventStream<'a> {
    cur_event: usize,
    num_events: usize,
    paths: &'a [*const libc::c_char],
    flags: &'a [u32],
    ids: &'a [u64],
}

/// An event received from the FSEvent API.
#[derive(Debug)]
struct FsEvent {
    path: PathBuf,
    flags: fse::StreamFlags,
    id: u64,
}

impl FsEventWatcher {
    pub fn new(paths: Vec<PathBuf>, queue: WaitQueue) -> Self {

        let runloop_ptr: *mut libc::c_void = ptr::null_mut();
        let runloop_ptr = Arc::new(atomic::AtomicPtr::new(runloop_ptr));
        let runloop_ptr2 = runloop_ptr.clone();

        let ptr_set_barrier = Arc::new(Barrier::new(2));
        let ptr_set_barrier2 = ptr_set_barrier.clone();


        thread::spawn(move || {

        let flags = fs::kFSEventStreamCreateFlagFileEvents;
        let paths = cf_array_from_pathbufs(paths);
        let latency = 0.0;
        let since_when = fs::kFSEventStreamEventIdSinceNow;

        let ctx = Box::new(Context::new(queue));
        let stream_ctx = fs::FSEventStreamContext {
            version: 0,
            info: unsafe { mem::transmute(Box::into_raw(ctx)) },
            retain: cf::NULL,
            copy_description: cf::NULL,
        };

        let cb = callback as *mut _;
        unsafe {
            let fse_stream = fs::FSEventStreamCreate(cf::kCFAllocatorDefault,
                                                     cb,
                                                     &stream_ctx,
                                                     paths,
                                                     since_when,
                                                     latency,
                                                     flags);

            let runloop = cf::CFRunLoopGetCurrent();

            runloop_ptr2.store(runloop as *mut libc::c_void,
                               atomic::Ordering::Relaxed);

            ptr_set_barrier2.wait();

            fs::FSEventStreamScheduleWithRunLoop(fse_stream, runloop,
                                                 cf::kCFRunLoopDefaultMode);
            fs::FSEventStreamStart(fse_stream);

            cf::CFRunLoopRun();
            // the previous call blocks until we cancel from another thread
            fs::FSEventStreamStop(fse_stream);
            fs::FSEventStreamInvalidate(fse_stream);
            fs::FSEventStreamRelease(fse_stream);
        }
        });

        ptr_set_barrier.wait();
        // don't return until the runloop pointer has been set.
        FsEventWatcher { runloop_ptr }
    }
}

impl Drop for FsEventWatcher {
    fn drop(&mut self) {
        let runloop_ptr = self.runloop_ptr.load(atomic::Ordering::Relaxed);
        assert!(!runloop_ptr.is_null());
        unsafe {
            while !CFRunLoopIsWaiting(runloop_ptr) {
                thread::yield_now();
            }
            cf::CFRunLoopStop(runloop_ptr);
        }
    }
}

impl HistoricalEvent {
    /// Returnes `true` if this historical event is from the same 'FSEvent epoch'
    /// as the new event.
    fn is_same(&self, event: &FsEvent, timestamp: Instant) -> bool {
        let elapsed = timestamp.duration_since(self.timestamp);
        let is_superset = (self.flags - event.flags).is_empty();
        is_superset && elapsed < Duration::from_secs(30)
    }
}

impl Context {
    fn new(queue: WaitQueue) -> Self {
        Context {
            queue: queue,
            history: HashMap::new(),
        }
    }

    fn enqueue_event(&self, event: Event) {
        let &(ref deque, ref cond) = &*self.queue;
        let mut deque = deque.lock().unwrap();
        deque.push_back(event);
        cond.notify_one();
    }

    fn enqueue_raw<O: Into<Option<usize>>>(&self, kind: EventKind,
                                           path: &Path,
                                           relid: O) {
        let paths = vec![path.to_owned()];
        let relid = relid.into();
        let event = Event { kind, paths, relid };
        self.enqueue_event(event);
    }

    fn get_and_update_prev(&mut self, event: &FsEvent, timestamp: Instant)
        -> Option<HistoricalEvent> {
        let prev = if let Some(prev) = self.history.get(&event.path) {
            if prev.is_same(event, timestamp) { None } else { Some(prev.to_owned()) }
        } else {
            None
        };
        // if there was an existing previous event we reuse its timestamp; fsevent
        // staleness is based on an internal timer, not a duration between events.
        self.history.entry(event.path.to_owned())
            .or_insert(
                HistoricalEvent {
                    timestamp: timestamp,
                    flags: *&event.flags,
                }).flags = *&event.flags;
        prev
    }

    fn send_all(&mut self, flags: fse::StreamFlags, path: &Path, id: usize) {
        use EventKind::*;
        use ModifyKind as MK;
        use RemoveKind as RK;
        use CreateKind as CK;
        use MetadataKind as MetK;

        if flags.contains(fse::ITEM_RENAMED) {
            self.enqueue_raw(Modify(MK::Name(RenameMode::Any)), path, id as usize);
        }

        if flags.contains(fse::ITEM_REMOVED) {
            let typ = if flags.contains(fse::IS_DIR) { RK::Folder } else { RK::File };
            self.enqueue_raw(Remove(typ), path, id as usize);
        }

        if flags.contains(fse::ITEM_CREATED) {
            let typ = if flags.contains(fse::IS_DIR) { CK::Folder } else { CK::File };
            self.enqueue_raw(Create(typ), path, id as usize);
        }

        if flags.contains(fse::ITEM_MODIFIED) || flags.contains(fse::INOTE_META_MOD) {
            self.enqueue_raw(Modify(MK::Data(DataChange::Any)), path, id as usize);
        }

        if flags.contains(fse::ITEM_CHANGE_OWNER) {
            self.enqueue_raw(Modify(MK::Metadata(MetK::Ownership)), path, id as usize);
        }

        if flags.contains(fse::ITEM_XATTR_MOD) {
            self.enqueue_raw(Modify(MK::Metadata(MetK::Other("xattrs".into()))),
                             path, id as usize);
        }

        if flags.contains(fse::FINDER_INFO_MOD) {
            self.enqueue_raw(Modify(MK::Metadata(MetK::Other("finder_info".into()))),
                             path, id as usize);
        }
    }
}

impl<'a> Iterator for EventStream<'a> {
    type Item = FsEvent;
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur_event == self.num_events { return None }
        let cur_event = self.cur_event;
        let path = unsafe {
            CStr::from_ptr(self.paths[cur_event]).to_bytes().to_owned()
        };
        let path: PathBuf = OsString::from_vec(path).into();
        let flags = fse::StreamFlags::from_bits(self.flags[cur_event])
            .expect("failed to decode streamflags");
        let id = self.ids[cur_event];

        self.cur_event += 1;
        Some(FsEvent { path, flags, id })
    }
}

fn cf_array_from_pathbufs(paths: Vec<PathBuf>) -> cf::CFMutableArrayRef {
    let cfarray = unsafe {
        cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 0,
                                 &cf::kCFTypeArrayCallBacks)
    };
    for path in paths {
        // NOTE: upstream expects this to be a str, but filepaths will
        // exist on running macs that are not valid UTF-8. Upstream should
        // probably take an OsString?
        let s = &path.to_str().unwrap();
        unsafe {
            let cf_path = cf::str_path_to_cfstring_ref(s);
            cf::CFArrayAppendValue(cfarray, cf_path);
            cf::CFRelease(cf_path);
        }
    }
    cfarray
}

#[allow(unused_variables)]
#[doc(hidden)]
pub unsafe extern "C" fn callback(stream_ref: fs::FSEventStreamRef,
                                  info: *mut libc::c_void,
                                  num_events: libc::size_t,
                                  event_paths: *const *const libc::c_char,
                                  event_flags: *mut libc::c_void,
                                  event_ids: *mut libc::c_void,
                                 ) {

    let num_events = num_events as usize;
    let event_flags = event_flags as *mut u32;
    let event_ids = event_ids as *mut u64;
    let ctx = mem::transmute::<_, *mut Context>(info);

    let paths = slice::from_raw_parts(event_paths, num_events);
    let flags = slice::from_raw_parts_mut(event_flags, num_events);
    let ids = slice::from_raw_parts_mut(event_ids, num_events);

    let cur_event = 0;
    let iter = EventStream { cur_event, num_events, paths, flags, ids };
    callback_impl(&mut (*ctx), iter);
}

fn callback_impl<'a>(ctx: &mut Context, stream: EventStream<'a>) {

    let recv_time = Instant::now();

    for event in stream {
        //eprintln!("fse_event {:?}", &event);
        let prev_event = ctx.get_and_update_prev(&event, recv_time);
        let FsEvent { path, flags, id } = event;

        if flags.contains(fse::MUST_SCAN_SUBDIRS) {
            ctx.enqueue_raw(EventKind::Other("rescan".into()), &path, None);
            continue;
        }

        match prev_event {
            None => ctx.send_all(flags, &path, id as usize),
            Some(prev_event) => {
                // if there are existing flags, we send coarse events
                let new_flags = flags - prev_event.flags;
                ctx.send_all(new_flags, &path, id as usize);
                ctx.enqueue_raw(EventKind::Any, &path, id as usize);
            }
        }

        // NOTE: we don't currently handle the unmount case, because we do
        // not currently pass the WatchRoot flag to FSEventStream.
        // we _probably_ want to do this, although it's unclear how
        // we should handle moving directories along our path, e.g. when
        // watching foo/bar, moving foo -> foo2 (so our path is foo2/bar)
    }
}

//TODO: this should be in the upstream crate
extern "C" {
    pub fn CFRunLoopIsWaiting(runloop: cf::CFRunLoopRef) -> bool;
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prev_event() {
        let p = PathBuf::from("/path/to/my/file.txt");
        let flags: fse::StreamFlags = fse::ITEM_CREATED;
        let timestamp = Instant::now() - Duration::from_secs(5);
        let mut hist = HistoricalEvent { timestamp, flags };

        let mut rel_event = FsEvent {
            path: p.clone(),
            flags: fse::ITEM_CREATED | fse::ITEM_MODIFIED,
            id: 42,
        };

        assert!(hist.is_same(&rel_event, Instant::now()));

        hist.flags = fse::INOTE_META_MOD | fse::ITEM_MODIFIED;
        assert!(!hist.is_same(&rel_event, Instant::now()));

        hist.flags = fse::ITEM_CREATED;
        assert!(hist.is_same(&rel_event, Instant::now()));

        hist.timestamp = Instant::now() - Duration::from_secs(31);
        assert!(!hist.is_same(&rel_event, Instant::now()));

        hist.timestamp = Instant::now() - Duration::from_secs(5);
        assert!(hist.is_same(&rel_event, Instant::now()));

        hist.flags = fse::ITEM_CREATED | fse::ITEM_MODIFIED;
        assert!(hist.is_same(&rel_event, Instant::now()));

        rel_event.flags = fse::ITEM_MODIFIED;
        assert!(!hist.is_same(&rel_event, Instant::now()));
    }
}
