use libc;
use std::path::PathBuf;
use std::sync::{atomic, Arc, Barrier};
use std::{slice, mem};
use std::ffi::{CStr, OsString};
use std::os::unix::ffi::OsStringExt;
use std::ptr;
use std::thread;

use backend::event::*;
use super::WaitQueue;

use fsevent_rs as fse;
use fsevent_sys::core_foundation as cf;
use fsevent_sys::fsevent as fs;

pub struct FsEventWatcher {
    runloop_ptr: Arc<atomic::AtomicPtr<libc::c_void>>,
}

struct Context {
    queue: WaitQueue,
}

struct EventStream<'a> {
    cur_event: usize,
    num_events: usize,
    paths: &'a [*const libc::c_char],
    flags: &'a [u32],
    ids: &'a [u64],
}

struct FsEvent {
    path: PathBuf,
    flags: fse::StreamFlags,
    id: u64,
}

impl FsEventWatcher {
    pub fn new(paths: Vec<PathBuf>, queue: WaitQueue) -> Self {

        let runloop_ptr: *mut libc::c_void = ptr::null_mut();
        let runloop_ptr = Arc::new(atomic::AtomicPtr::new(runloop_ptr));
        let ptr_box = runloop_ptr.clone();

        let ptr_set_barrier = Arc::new(Barrier::new(2));
        let ptr_set_barrier2 = ptr_set_barrier.clone();


        thread::spawn(move || {

        let flags = fs::kFSEventStreamCreateFlagFileEvents;
        let paths = cf_array_from_pathbufs(paths);
        let latency = 0.0;
        let since_when = fs::kFSEventStreamEventIdSinceNow;

            let ctx = Box::new(Context { queue });
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

                ptr_box.store(runloop as *mut libc::c_void,
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

impl Context {
    fn enqueue_event(&self, event: Event) {
        let &(ref deque, ref cond) = &*self.queue;
        let mut deque = deque.lock().unwrap();
        deque.push_back(event);
        cond.notify_one();
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
        // probably take a &[u8]?
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
    let ctx = mem::transmute::<_, *const Context>(info);

    let paths = slice::from_raw_parts(event_paths, num_events);
    let flags = slice::from_raw_parts_mut(event_flags, num_events);
    let ids = slice::from_raw_parts_mut(event_ids, num_events);

    let cur_event = 0;
    let iter = EventStream { cur_event, num_events, paths, flags, ids };
    callback_impl(&(*ctx), iter);
}

fn callback_impl<'a>(ctx: &Context, stream: EventStream<'a>) {
    use EventKind::*;
    use ModifyKind as MK;
    use RemoveKind as RK;
    use CreateKind as CK;
    use MetadataKind as MetK;

    for event in stream {
        let FsEvent { path, flags, id } = event;
        let kind = match flags {
            f if f.contains(fse::MUST_SCAN_SUBDIRS) =>
                Some(Other("rescan".into())),

            f if f.contains(fse::ITEM_RENAMED) =>
                Some(Modify(MK::Name(RenameMode::Any))),

            f if f.contains(fse::ITEM_REMOVED) => {
                let typ = if f.contains(fse::IS_DIR) { RK::Folder } else { RK::File };
                Some(Remove(typ))
            }

            // create events may contain 'modify' flag, but modify events
            // always contain 'inode meta mod'?
            f if f.contains(fse::ITEM_CREATED) && !f.contains(fse::INOTE_META_MOD) => {
                let typ = if f.contains(fse::IS_DIR) { CK::Folder } else { CK::File };
                Some(Create(typ))
            }

            // modify without create is always modify
            f if f.contains(fse::ITEM_MODIFIED) && !f.contains(fse::ITEM_CREATED) =>
                Some(Modify(MK::Data(DataChange::Any))),

                // modify with inode meta mod, with our w/o create, is modify
            f if f.contains(fse::ITEM_MODIFIED) && f.contains(fse::INOTE_META_MOD) =>
                Some(Modify(MK::Data(DataChange::Any))),

            f if f.contains(fse::ITEM_CHANGE_OWNER) =>
                Some(Modify(MK::Metadata(MetK::Ownership))),

            f if f.contains(fse::ITEM_XATTR_MOD) =>
                Some(Modify(MK::Metadata(MetK::Other("attributes".into())))),

            f if f.contains(fse::FINDER_INFO_MOD) =>
                Some(Modify(MK::Metadata(MetK::Other("finder_info".into())))),

            // NOTE: we don't currently handle the unmount case, because we do
            // not currently pass the WatchRoot flag to FSEventStream.
            // we _probably_ want to do this, although it's unclear how
            // we should handle moving directories along our path, e.g. when
            // watching foo/bar, moving foo -> foo2 (so our path is foo2/bar)
            other => {
                eprintln!("ignoring flags {:?} for path {:?}", flags, &path);
                None
            }
        };

        if let Some(kind) = kind {

            let event = Event {
                kind: kind,
                paths: vec![path],
                relid: None,
            };

            ctx.enqueue_event(event);
        }
    }
}

//TODO: this should be in the upstream crate
extern "C" {
    pub fn CFRunLoopIsWaiting(runloop: cf::CFRunLoopRef) -> bool;
}

