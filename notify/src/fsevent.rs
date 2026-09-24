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

use crate::consolidating_path_trie::ConsolidatingPathTrie;
use crate::{
    Config, Error, ErrorKind, EventHandler, PathsMut, Result, Sender, WatchMode, Watcher, unbounded,
};
use crate::{TargetMode, event::*};
use objc2_core_foundation as cf;
use objc2_core_services as fs;
use rustc_hash::FxBuildHasher;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, OsStr};
use std::fmt;
use std::hash::RandomState;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

bitflags::bitflags! {
  #[repr(C)]
  #[derive(Debug)]
  struct StreamFlags: u32 {
    const NONE = fs::kFSEventStreamEventFlagNone;
    const MUST_SCAN_SUBDIRS = fs::kFSEventStreamEventFlagMustScanSubDirs;
    const USER_DROPPED = fs::kFSEventStreamEventFlagUserDropped;
    const KERNEL_DROPPED = fs::kFSEventStreamEventFlagKernelDropped;
    const IDS_WRAPPED = fs::kFSEventStreamEventFlagEventIdsWrapped;
    const HISTORY_DONE = fs::kFSEventStreamEventFlagHistoryDone;
    const ROOT_CHANGED = fs::kFSEventStreamEventFlagRootChanged;
    const MOUNT = fs::kFSEventStreamEventFlagMount;
    const UNMOUNT = fs::kFSEventStreamEventFlagUnmount;
    const ITEM_CREATED = fs::kFSEventStreamEventFlagItemCreated;
    const ITEM_REMOVED = fs::kFSEventStreamEventFlagItemRemoved;
    const INODE_META_MOD = fs::kFSEventStreamEventFlagItemInodeMetaMod;
    const ITEM_RENAMED = fs::kFSEventStreamEventFlagItemRenamed;
    const ITEM_MODIFIED = fs::kFSEventStreamEventFlagItemModified;
    const FINDER_INFO_MOD = fs::kFSEventStreamEventFlagItemFinderInfoMod;
    const ITEM_CHANGE_OWNER = fs::kFSEventStreamEventFlagItemChangeOwner;
    const ITEM_XATTR_MOD = fs::kFSEventStreamEventFlagItemXattrMod;
    const IS_FILE = fs::kFSEventStreamEventFlagItemIsFile;
    const IS_DIR = fs::kFSEventStreamEventFlagItemIsDir;
    const IS_SYMLINK = fs::kFSEventStreamEventFlagItemIsSymlink;
    const OWN_EVENT = fs::kFSEventStreamEventFlagOwnEvent;
    const IS_HARDLINK = fs::kFSEventStreamEventFlagItemIsHardlink;
    const IS_LAST_HARDLINK = fs::kFSEventStreamEventFlagItemIsLastHardlink;
    const ITEM_CLONED = fs::kFSEventStreamEventFlagItemCloned;
  }
}

/// FSEvents-based `Watcher` implementation
pub struct FsEventWatcher {
    paths: cf::CFRetained<cf::CFMutableArray<cf::CFString>>,
    since_when: fs::FSEventStreamEventId,
    latency: cf::CFTimeInterval,
    flags: fs::FSEventStreamCreateFlags,
    event_handler: Arc<Mutex<dyn EventHandler>>,
    runloop: Option<(cf::CFRetained<cf::CFRunLoop>, thread::JoinHandle<()>)>,
    watches: HashMap<PathBuf, bool, FxBuildHasher>,
    max_fsevent_paths: usize,
}

// FSEvents applies the path limit across live streams, so all watcher instances
// in this process must share the same count.
static ACTIVE_FSEVENTS_PATHS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct FseventsPathReservation {
    active_paths: &'static AtomicUsize,
    path_count: usize,
}

impl FseventsPathReservation {
    fn acquire(
        active_paths: &'static AtomicUsize,
        path_count: usize,
        budget: usize,
    ) -> std::result::Result<Self, usize> {
        active_paths
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active_path_count| {
                active_path_count
                    .checked_add(path_count)
                    .filter(|&combined_path_count| combined_path_count <= budget)
            })
            .map(|_| Self {
                active_paths,
                path_count,
            })
    }
}

impl Drop for FseventsPathReservation {
    fn drop(&mut self) {
        let previous = self
            .active_paths
            .fetch_sub(self.path_count, Ordering::Relaxed);
        debug_assert!(previous >= self.path_count);
    }
}

impl fmt::Debug for FsEventWatcher {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FsEventWatcher")
            .field("paths", &self.paths)
            .field("since_when", &self.since_when)
            .field("latency", &self.latency)
            .field("flags", &self.flags)
            .field("event_handler", &Arc::as_ptr(&self.event_handler))
            .field("runloop", &self.runloop)
            .field("watches", &self.watches)
            .finish()
    }
}

// CFMutableArrayRef is a type alias to *mut libc::c_void, so FsEventWatcher is not Send/Sync
// automatically. It's Send because the pointer is not used in other threads.
unsafe impl Send for FsEventWatcher {}

// It's Sync because all methods that change the mutable state use `&mut self`.
unsafe impl Sync for FsEventWatcher {}

#[expect(clippy::too_many_lines)]
fn translate_flags(flags: &StreamFlags, precise: bool, root_path: Option<bool>) -> Vec<Event> {
    let mut evs = Vec::new();
    translate_flags_with(flags, precise, root_path, |ev| evs.push(ev));
    evs
}

// Keep this in sync with `translate_flags_with`; the callback uses it to avoid path clones.
fn translated_event_count(flags: &StreamFlags, precise: bool) -> usize {
    if flags.contains(StreamFlags::HISTORY_DONE) {
        return 0;
    }

    let mut count = usize::from(flags.contains(StreamFlags::MUST_SCAN_SUBDIRS));
    if !precise {
        return count + 1;
    }

    let root_changed = flags.contains(StreamFlags::ROOT_CHANGED);
    count += usize::from(root_changed);
    count += usize::from(flags.contains(StreamFlags::MOUNT));
    count += usize::from(flags.contains(StreamFlags::UNMOUNT));
    count += usize::from(flags.contains(StreamFlags::ITEM_CREATED));
    count += usize::from(flags.contains(StreamFlags::ITEM_RENAMED) && !root_changed);
    count += usize::from(flags.contains(StreamFlags::INODE_META_MOD));
    count += usize::from(flags.contains(StreamFlags::FINDER_INFO_MOD));
    count += usize::from(flags.contains(StreamFlags::ITEM_CHANGE_OWNER));
    count += usize::from(flags.contains(StreamFlags::ITEM_XATTR_MOD));
    count += usize::from(flags.contains(StreamFlags::ITEM_MODIFIED));
    count += usize::from(flags.contains(StreamFlags::ITEM_REMOVED) && !root_changed);
    count
}

/// `root_path` says, for a `ROOT_CHANGED` event, whether the root exists now and is a directory.
fn translate_flags_with(
    flags: &StreamFlags,
    precise: bool,
    root_path: Option<bool>,
    mut emit: impl FnMut(Event),
) {
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
    if flags.contains(StreamFlags::HISTORY_DONE) {
        return;
    }

    // `ITEM_CLONED` can be present alongside other flags (including create/modify/remove).
    // Preserve any existing `info` (like "root changed"), but annotate otherwise so downstream
    // can detect and filter clone-related events. See https://github.com/notify-rs/notify/issues/465.
    let clone_related = precise && flags.contains(StreamFlags::ITEM_CLONED);
    let own_process_id = if precise && flags.contains(StreamFlags::OWN_EVENT) {
        Some(std::process::id())
    } else {
        None
    };

    let mut emit_event = |mut ev: Event| {
        if clone_related && ev.info().is_none() {
            ev.attrs.set_info("is: clone");
        }
        if let Some(process_id) = own_process_id {
            ev.attrs.set_process_id(process_id);
        }
        emit(ev);
    };

    // FSEvents provides two possible hints as to why events were dropped,
    // however documentation on what those mean is scant, so we just pass them
    // through in the info attr field. The intent is clear enough, and the
    // additional information is provided if the user wants it.
    if flags.contains(StreamFlags::MUST_SCAN_SUBDIRS) {
        let e = Event::new(EventKind::Other).set_flag(Flag::Rescan);
        emit_event(if flags.contains(StreamFlags::USER_DROPPED) {
            e.set_info("rescan: user dropped")
        } else if flags.contains(StreamFlags::KERNEL_DROPPED) {
            e.set_info("rescan: kernel dropped")
        } else {
            e
        });
    }

    // In imprecise mode, let's not even bother parsing the kind of the event
    // except for the above very special events.
    if !precise {
        emit(Event::new(EventKind::Any));
        return;
    }

    // A watched root changed: it, or a directory above it, was renamed, removed or brought back.
    // The disk says which. A root that is there is reported as created, unless the flags carry
    // its own creation, which is reported below. For a root that is gone, the flags say whether
    // it was renamed; otherwise it is treated as removed rather than guessed to be renamed.
    let root_changed = flags.contains(StreamFlags::ROOT_CHANGED);
    if root_changed {
        match root_path {
            Some(is_dir) => {
                if !flags.contains(StreamFlags::ITEM_CREATED) {
                    let kind = if is_dir {
                        CreateKind::Folder
                    } else {
                        CreateKind::File
                    };
                    emit_event(Event::new(EventKind::Create(kind)).set_info("root changed"));
                }
            }
            None => {
                let kind = if flags.contains(StreamFlags::ITEM_REMOVED) {
                    if flags.contains(StreamFlags::IS_DIR) {
                        EventKind::Remove(RemoveKind::Folder)
                    } else if flags.contains(StreamFlags::IS_FILE) {
                        EventKind::Remove(RemoveKind::File)
                    } else {
                        EventKind::Remove(RemoveKind::Any)
                    }
                } else if flags.contains(StreamFlags::ITEM_RENAMED) {
                    EventKind::Modify(ModifyKind::Name(RenameMode::From))
                } else {
                    EventKind::Remove(RemoveKind::Any)
                };
                emit_event(Event::new(kind).set_info("root changed"));
            }
        }
    }

    // A path was mounted at the event path; we treat that as a create.
    if flags.contains(StreamFlags::MOUNT) {
        emit_event(Event::new(EventKind::Create(CreateKind::Other)).set_info("mount"));
    }

    // A path was unmounted at the event path; we treat that as a remove.
    if flags.contains(StreamFlags::UNMOUNT) {
        emit_event(Event::new(EventKind::Remove(RemoveKind::Other)).set_info("mount"));
    }

    if flags.contains(StreamFlags::ITEM_CREATED) {
        emit_event(if flags.contains(StreamFlags::IS_DIR) {
            Event::new(EventKind::Create(CreateKind::Folder))
        } else if flags.contains(StreamFlags::IS_FILE) {
            Event::new(EventKind::Create(CreateKind::File))
        } else {
            let e = Event::new(EventKind::Create(CreateKind::Other));
            if flags.contains(StreamFlags::IS_SYMLINK) {
                e.set_info("is: symlink")
            } else if flags.contains(StreamFlags::IS_HARDLINK) {
                e.set_info("is: hardlink")
            } else if flags.contains(StreamFlags::ITEM_CLONED) {
                e.set_info("is: clone")
            } else {
                Event::new(EventKind::Create(CreateKind::Any))
            }
        });
    }

    // FSEvents provides no mechanism to associate the old and new sides of a
    // rename event.
    // Avoid emitting duplicate events around a root change by checking `root_changed`.
    if flags.contains(StreamFlags::ITEM_RENAMED) && !root_changed {
        emit_event(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::Any,
        ))));
    }

    // This is only described as "metadata changed", but it may be that it's
    // only emitted for some more precise subset of events... if so, will need
    // amending, but for now we have an Any-shaped bucket to put it in.
    if flags.contains(StreamFlags::INODE_META_MOD) {
        emit_event(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Any,
        ))));
    }

    if flags.contains(StreamFlags::FINDER_INFO_MOD) {
        emit_event(
            Event::new(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Other)))
                .set_info("meta: finder info"),
        );
    }

    if flags.contains(StreamFlags::ITEM_CHANGE_OWNER) {
        emit_event(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Ownership,
        ))));
    }

    if flags.contains(StreamFlags::ITEM_XATTR_MOD) {
        emit_event(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Extended,
        ))));
    }

    // This is specifically described as a data change, which we take to mean
    // is a content change.
    if flags.contains(StreamFlags::ITEM_MODIFIED) {
        emit_event(Event::new(EventKind::Modify(ModifyKind::Data(
            DataChange::Content,
        ))));
    }

    // Avoid emitting duplicate events around a root change by checking `root_changed`.
    if flags.contains(StreamFlags::ITEM_REMOVED) && !root_changed {
        emit_event(if flags.contains(StreamFlags::IS_DIR) {
            Event::new(EventKind::Remove(RemoveKind::Folder))
        } else if flags.contains(StreamFlags::IS_FILE) {
            Event::new(EventKind::Remove(RemoveKind::File))
        } else {
            let e = Event::new(EventKind::Remove(RemoveKind::Other));
            if flags.contains(StreamFlags::IS_SYMLINK) {
                e.set_info("is: symlink")
            } else if flags.contains(StreamFlags::IS_HARDLINK) {
                e.set_info("is: hardlink")
            } else if flags.contains(StreamFlags::ITEM_CLONED) {
                e.set_info("is: clone")
            } else {
                Event::new(EventKind::Remove(RemoveKind::Any))
            }
        });
    }
}

struct StreamContextInfo {
    event_handler: Arc<Mutex<dyn EventHandler>>,
    recursive_info: HashMap<PathBuf, bool, FxBuildHasher>,
}

// Free the context when the stream created by `FSEventStreamCreate` is released.
extern "C-unwind" fn release_context(info: *const libc::c_void) {
    // Safety:
    // - The [documentation] for `FSEventStreamContext` states that `release` is only
    //   called when the stream is deallocated, so it is safe to convert `info` back into a
    //   box and drop it.
    //
    // [docs]: https://developer.apple.com/documentation/coreservices/fseventstreamcontext?language=objc
    unsafe {
        drop(Box::from_raw(info.cast::<StreamContextInfo>().cast_mut()));
    }
}

struct FsEventPathsMut<'a>(&'a mut FsEventWatcher);
impl<'a> FsEventPathsMut<'a> {
    fn new(watcher: &'a mut FsEventWatcher) -> Self {
        watcher.stop();
        Self(watcher)
    }
}
impl PathsMut for FsEventPathsMut<'_> {
    #[tracing::instrument(level = "debug", skip(self))]
    fn add(&mut self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        self.0.append_path(path, watch_mode)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn remove(&mut self, path: &Path) -> Result<()> {
        self.0.remove_path(path)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn commit(self: Box<Self>) -> Result<()> {
        self.0.run()
    }
}

impl FsEventWatcher {
    fn from_event_handler(
        event_handler: Arc<Mutex<dyn EventHandler>>,
        max_fsevent_paths: usize,
    ) -> Self {
        FsEventWatcher {
            paths: cf::CFMutableArray::empty(),
            since_when: fs::kFSEventStreamEventIdSinceNow,
            latency: 0.0,
            flags: fs::kFSEventStreamCreateFlagFileEvents
                | fs::kFSEventStreamCreateFlagNoDefer
                | fs::kFSEventStreamCreateFlagWatchRoot,
            event_handler,
            runloop: None,
            watches: HashMap::default(),
            max_fsevent_paths,
        }
    }

    fn watch_inner(&mut self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        self.stop();
        let result = self.append_path(path, watch_mode);
        self.run()?;
        result
    }

    fn unwatch_inner(&mut self, path: &Path) -> Result<()> {
        self.stop();
        let result = self.remove_path(path);
        self.run()?;
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

        if let Some((runloop, thread_handle)) = self.runloop.take() {
            while !runloop.is_waiting() {
                thread::yield_now();
            }

            runloop.stop();

            // Wait for the thread to shut down.
            thread_handle.join().expect("thread to shut down");
        }
    }

    fn remove_path(&mut self, path: &Path) -> Result<()> {
        let p = if let Ok(canonicalized_path) = path.canonicalize() {
            canonicalized_path
        } else {
            path.to_owned()
        };
        match self.watches.remove(&p) {
            Some(_) => Ok(()),
            None => Err(Error::watch_not_found()),
        }
    }

    // https://github.com/thibaudgg/rb-fsevent/blob/master/ext/fsevent_watch/main.c
    fn append_path(&mut self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        if (!path.exists() && watch_mode.target_mode != TargetMode::TrackPath)
            || path == Path::new("")
        {
            return Err(Error::path_not_found().add_path(path.into()));
        }
        let canonical_path = path
            .to_path_buf()
            .canonicalize()
            .unwrap_or(path.to_path_buf());

        self.watches
            .insert(canonical_path, watch_mode.recursive_mode.is_recursive());
        Ok(())
    }

    fn update_paths_based_on_watches(&mut self) {
        let paths_to_watch = {
            let mut trie = ConsolidatingPathTrie::new(true, self.max_fsevent_paths);
            for path in self.watches.keys() {
                trie.insert(path.clone());
            }
            trie.values()
        };
        tracing::debug!("Watching the following paths: {paths_to_watch:?}");
        let paths_to_watch_set = paths_to_watch
            .iter()
            .map(|p| p.to_string_lossy().to_lowercase())
            .collect::<HashSet<_>>();
        let mut already_included_paths =
            HashSet::<String, RandomState>::with_capacity(self.paths.len());

        // remove no longer watched paths
        let mut to_remove = Vec::new();
        for (idx, item) in self.paths.iter().enumerate() {
            if paths_to_watch_set.contains(&item.to_string()) {
                already_included_paths.insert(item.to_string());
            } else {
                to_remove.push(cf::CFIndex::try_from(idx).unwrap());
            }
        }
        for idx in to_remove.iter().rev() {
            // SAFETY: `the_array` is not `None` and the generic is correct, `idx` is in-bounds
            unsafe {
                cf::CFMutableArray::remove_value_at_index(Some(self.paths.as_opaque()), *idx);
            };
        }

        // add new paths
        for path in paths_to_watch {
            if !already_included_paths.contains(&path.to_string_lossy().to_lowercase()) {
                self.paths
                    .append(&cf::CFString::from_str(&path.to_string_lossy()));
            }
        }
    }

    fn run(&mut self) -> Result<()> {
        if self.watches.is_empty() {
            return Ok(());
        }

        self.update_paths_based_on_watches();

        // Over roughly RLIMIT_NOFILE/10 paths across all live streams, FSEvents
        // closes fd 0, which this process owns. The corruption then surfaces as
        // EBADF on unrelated files.
        let path_count = self.paths.iter().count();
        let budget = fsevents_path_budget().unwrap_or(usize::MAX);
        let path_reservation =
            match FseventsPathReservation::acquire(&ACTIVE_FSEVENTS_PATHS, path_count, budget) {
                Ok(reservation) => reservation,
                Err(active_path_count) => {
                    let combined_path_count = active_path_count.saturating_add(path_count);
                    tracing::error!(
                        "refusing FSEvents stream: {combined_path_count} active paths exceed the \
                         safe limit of {budget}. Raise RLIMIT_NOFILE, watch fewer paths, or use \
                         macos_kqueue."
                    );
                    return Err(Error::new(ErrorKind::MaxFilesWatch));
                }
            };

        // We need to associate the stream context with our callback in order to propagate events
        // to the rest of the system. This will be owned by the stream, and will be freed when the
        // stream is closed. This means we will leak the context if we panic before reaching
        // `FSEventStreamRelease`.
        let context = Box::into_raw(Box::new(StreamContextInfo {
            event_handler: Arc::clone(&self.event_handler),
            recursive_info: self.watches.clone(),
        }));

        let mut stream_context = fs::FSEventStreamContext {
            version: 0,
            info: context.cast::<libc::c_void>(),
            retain: None,
            release: Some(release_context),
            copyDescription: None,
        };

        let stream = unsafe {
            fs::FSEventStreamCreate(
                cf::kCFAllocatorDefault,
                Some(callback),
                &raw mut stream_context,
                self.paths.as_opaque(),
                self.since_when,
                self.latency,
                self.flags,
            )
        };

        // Wrapper to help send CFRunLoop types across threads.
        struct CFRunLoopSendWrapper(cf::CFRetained<cf::CFRunLoop>);
        // Safety:
        // - According to the Apple documentation, it's safe to move `CFRunLoop`s across threads.
        //   https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Multithreading/ThreadSafetySummary/ThreadSafetySummary.html
        unsafe impl Send for CFRunLoopSendWrapper {}

        // Wrapper to help send FSEventStreamRef types across threads.
        struct FSEventStreamSendWrapper(fs::FSEventStreamRef);
        // SAFETY: Unclear?
        unsafe impl Send for FSEventStreamSendWrapper {}

        // move into thread
        let stream = FSEventStreamSendWrapper(stream);

        // channel to pass runloop around
        let (rl_tx, rl_rx) = unbounded();

        let thread_handle = thread::Builder::new()
            .name("notify-rs fsevents loop".to_string())
            .spawn(move || {
                // Keep the shared path count reserved until this stream is released.
                let _path_reservation = path_reservation;
                let _ = &stream;
                let stream = stream.0;

                unsafe {
                    // CFRunLoop::current() returns None only in OOM situations
                    let cur_runloop = cf::CFRunLoop::current().unwrap();

                    #[expect(deprecated)]
                    fs::FSEventStreamScheduleWithRunLoop(
                        stream,
                        &cur_runloop,
                        cf::kCFRunLoopDefaultMode.unwrap(),
                    );
                    if !fs::FSEventStreamStart(stream) {
                        fs::FSEventStreamInvalidate(stream);
                        fs::FSEventStreamRelease(stream);
                        rl_tx
                            .send(Err(Error::generic("unable to start FSEvent stream")))
                            .expect("Unable to send error for FSEventStreamStart");
                        return;
                    }

                    // the calling to CFRunLoopRun will be terminated by CFRunLoopStop call in drop()
                    rl_tx
                        .send(Ok(CFRunLoopSendWrapper(cur_runloop)))
                        .expect("Unable to send runloop to watcher");

                    cf::CFRunLoop::run();
                    fs::FSEventStreamStop(stream);
                    // There are edge-cases, when many events are pending,
                    // despite the stream being stopped, that the stream's
                    // associated callback will be invoked. Purging events
                    // is intended to prevent this.
                    let event_id = fs::FSEventsGetCurrentEventId();
                    let device = fs::FSEventStreamGetDeviceBeingWatched(stream);
                    if !fs::FSEventsPurgeEventsForDeviceUpToEventId(device, event_id) {
                        tracing::error!(
                            "FSEventsPurgeEventsForDeviceUpToEventId failed for device {device}, event id {event_id}",
                        );
                    }
                    fs::FSEventStreamInvalidate(stream);
                    fs::FSEventStreamRelease(stream);
                }
            })?;
        // block until runloop has been sent
        let runloop_wrapper = rl_rx.recv().unwrap()?;
        self.runloop = Some((runloop_wrapper.0, thread_handle));

        Ok(())
    }

    fn configure_raw_mode(_config: Config, tx: &Sender<Result<bool>>) {
        tx.send(Ok(false))
            .expect("configuration channel disconnect");
    }
}

// A twelfth rather than a tenth: the edge also shifts with how many descriptors
// the process already holds.
fn fsevents_path_budget() -> Option<usize> {
    let mut limit = unsafe { std::mem::zeroed::<libc::rlimit>() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) } != 0 {
        return None;
    }
    let soft = usize::try_from(limit.rlim_cur).ok()?;
    Some(soft / 12)
}

extern "C-unwind" fn callback(
    stream_ref: fs::ConstFSEventStreamRef,
    info: *mut libc::c_void,
    num_events: libc::size_t,                          // size_t numEvents
    event_paths: NonNull<libc::c_void>,                // void *eventPaths
    event_flags: NonNull<fs::FSEventStreamEventFlags>, // const FSEventStreamEventFlags eventFlags[]
    event_ids: NonNull<fs::FSEventStreamEventId>,      // const FSEventStreamEventId eventIds[]
) {
    // Never unwind into CoreServices; if something goes wrong, drop the events and log.
    // This also protects against panics from user-provided `EventHandler` implementations.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        callback_impl(
            stream_ref,
            info,
            num_events,
            event_paths,
            event_flags,
            event_ids,
        );
    }))
    .map_err(|_| {
        tracing::error!("panic in FSEvents callback; dropping pending events");
    });
}

unsafe fn callback_impl(
    _stream_ref: fs::ConstFSEventStreamRef,
    info: *mut libc::c_void,
    num_events: libc::size_t,                          // size_t numEvents
    event_paths: NonNull<libc::c_void>,                // void *eventPaths
    event_flags: NonNull<fs::FSEventStreamEventFlags>, // const FSEventStreamEventFlags eventFlags[]
    _event_ids: NonNull<fs::FSEventStreamEventId>,     // const FSEventStreamEventId eventIds[]
) {
    let event_paths = event_paths.as_ptr() as *const *const libc::c_char;
    let info = info as *const StreamContextInfo;
    let event_handler_mutex = unsafe { &(*info).event_handler };
    let mut event_handler_guard = None;

    for p in 0..num_events {
        // Paths are not guaranteed to be valid UTF-8 (e.g. NFS); keep them as raw bytes.
        let path = unsafe { CStr::from_ptr(*event_paths.add(p)) };
        let path = Path::new(OsStr::from_bytes(path.to_bytes()));

        let raw_flag = unsafe { *event_flags.as_ptr().add(p) };
        let flag = StreamFlags::from_bits_truncate(raw_flag);
        let unknown_bits = raw_flag & !StreamFlags::all().bits();
        if unknown_bits != 0 {
            // `FSEventStreamEventFlags` is an extensible bitfield; tolerate future flags.
            tracing::trace!("unknown FSEventStreamEventFlags bits: 0x{unknown_bits:08x}");
        }

        tracing::trace!(
            target = "rolldown-notify::fsevent::details",
            ?path,
            ?flag,
            "FSEvent raw event received"
        );

        let mut handle_event = false;
        for (watch_path, r) in unsafe { &(*info).recursive_info } {
            if path.starts_with(watch_path) {
                if *r || &path == watch_path {
                    handle_event = true;
                    break;
                } else if let Some(parent_path) = path.parent()
                    && parent_path == watch_path
                {
                    handle_event = true;
                    break;
                }
            }
        }

        if !handle_event {
            continue;
        }

        tracing::trace!(?path, ?flag, "FSEvent event received");

        let translated_count = translated_event_count(&flag, true);
        if translated_count == 0 {
            continue;
        }

        let root_path = if flag.contains(StreamFlags::ROOT_CHANGED) {
            std::fs::metadata(&path).ok().map(|meta| meta.is_dir())
        } else {
            None
        };
        translate_flags_with(&flag, true, root_path, |mut ev| {
            ev.paths.push(path.to_path_buf());

            let event_handler =
                event_handler_guard.get_or_insert_with(|| match event_handler_mutex.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                });
            // Protect against panicking event handlers, which would otherwise unwind into
            // the CoreServices callback.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                event_handler.handle_event(Ok(ev));
            }))
            .map_err(|_| {
                tracing::error!("panic in FSEvents event handler; dropping event");
            });
        });
    }
}

impl Watcher for FsEventWatcher {
    /// Create a new watcher.
    #[tracing::instrument(level = "debug", skip(event_handler))]
    fn new<F: EventHandler>(event_handler: F, config: Config) -> Result<Self> {
        Ok(Self::from_event_handler(
            Arc::new(Mutex::new(event_handler)),
            config.max_fsevent_paths(),
        ))
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn watch(&mut self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        self.watch_inner(path, watch_mode)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn paths_mut<'me>(&'me mut self) -> Box<dyn PathsMut + 'me> {
        Box::new(FsEventPathsMut::new(self))
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.unwatch_inner(path)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn configure(&mut self, config: Config) -> Result<bool> {
        let (tx, rx) = unbounded();
        Self::configure_raw_mode(config, &tx);
        rx.recv()?
    }

    fn kind() -> crate::WatcherKind {
        crate::WatcherKind::Fsevent
    }
}

impl Drop for FsEventWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{ErrorKind, RecursiveMode, TargetMode};

    use super::*;
    use crate::test::*;

    fn watcher() -> (TestWatcher<FsEventWatcher>, Receiver) {
        channel()
    }

    #[expect(clippy::print_stdout)]
    #[test]
    fn test_fsevent_watcher_drop() {
        use super::*;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();

        let (tx, rx) = std::sync::mpsc::channel();

        {
            let mut watcher = FsEventWatcher::new(tx, Config::default()).unwrap();
            watcher.watch(dir.path(), WatchMode::recursive()).unwrap();
            thread::sleep(Duration::from_millis(2000));
            println!("is running -> {}", watcher.is_running());

            thread::sleep(Duration::from_millis(1000));
            watcher.unwatch(dir.path()).unwrap();
            println!("is running -> {}", watcher.is_running());
        }

        thread::sleep(Duration::from_millis(1000));

        for res in rx {
            let e = res.unwrap();
            println!("debug => {:?} {:?}", e.kind, e.paths);
        }

        println!("in test: {} works", file!());
    }

    #[test]
    fn test_steam_context_info_send_and_sync() {
        fn check_send<T: Send + Sync>() {}
        check_send::<StreamContextInfo>();
    }

    #[test]
    fn callback_impl_handles_non_utf8_paths_without_panicking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::ptr;

        let (tx, rx) = std::sync::mpsc::channel::<crate::Result<Event>>();
        let event_handler: Arc<Mutex<dyn EventHandler>> = Arc::new(Mutex::new(tx));

        let mut recursive_info = HashMap::default();
        recursive_info.insert(PathBuf::from("/tmp"), true);

        let context = Box::new(StreamContextInfo {
            event_handler,
            recursive_info,
        });
        let context_ptr = Box::into_raw(context) as *mut libc::c_void;

        let bytes = b"/tmp/\xff";
        let c_path = CString::new(bytes.as_slice()).expect("cstring");
        let path_ptrs = [c_path.as_ptr()];
        let event_paths = NonNull::new(path_ptrs.as_ptr() as *mut libc::c_void).unwrap();

        let flags_arr = [StreamFlags::ITEM_CREATED.bits() as fs::FSEventStreamEventFlags];
        let event_flags =
            NonNull::new(flags_arr.as_ptr() as *mut fs::FSEventStreamEventFlags).unwrap();

        let ids_arr = [0 as fs::FSEventStreamEventId];
        let event_ids = NonNull::new(ids_arr.as_ptr() as *mut fs::FSEventStreamEventId).unwrap();

        let res = std::panic::catch_unwind(|| unsafe {
            callback_impl(
                ptr::null(),
                context_ptr,
                1,
                event_paths,
                event_flags,
                event_ids,
            );
        });
        unsafe {
            drop(Box::from_raw(context_ptr as *mut StreamContextInfo));
        }

        assert!(res.is_ok(), "callback_impl should not panic");

        let event = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("expected event")
            .expect("expected Ok(Event)");
        assert!(
            event.kind.is_create(),
            "expected create event, got {event:?}"
        );
        assert_eq!(event.paths.len(), 1);
        assert_eq!(event.paths[0].as_os_str().as_bytes(), bytes);
    }

    #[test]
    fn callback_impl_ignores_unknown_flag_bits_without_panicking() {
        use std::ffi::CString;
        use std::ptr;

        let (tx, rx) = std::sync::mpsc::channel::<crate::Result<Event>>();
        let event_handler: Arc<Mutex<dyn EventHandler>> = Arc::new(Mutex::new(tx));

        let mut recursive_info = HashMap::default();
        recursive_info.insert(PathBuf::from("/tmp"), true);

        let context = Box::new(StreamContextInfo {
            event_handler,
            recursive_info,
        });
        let context_ptr = Box::into_raw(context) as *mut libc::c_void;

        let c_path = CString::new("/tmp/file").expect("cstring");
        let path_ptrs = [c_path.as_ptr()];
        let event_paths = NonNull::new(path_ptrs.as_ptr() as *mut libc::c_void).unwrap();

        // Include an unknown bit so the old `from_bits(...).unwrap_or_else(panic!)` behavior
        // would have panicked. New behavior should tolerate it.
        let unknown_mask = !StreamFlags::all().bits();
        let unknown_bit = unknown_mask & unknown_mask.wrapping_neg();
        assert_ne!(unknown_bit, 0, "StreamFlags unexpectedly uses all bits");
        let raw_flag = StreamFlags::ITEM_CREATED.bits() | unknown_bit;
        assert!(
            StreamFlags::from_bits(raw_flag).is_none(),
            "raw_flag must include an unknown bit for this test to be meaningful"
        );

        let flags_arr = [raw_flag as fs::FSEventStreamEventFlags];
        let event_flags =
            NonNull::new(flags_arr.as_ptr() as *mut fs::FSEventStreamEventFlags).unwrap();

        let ids_arr = [0 as fs::FSEventStreamEventId];
        let event_ids = NonNull::new(ids_arr.as_ptr() as *mut fs::FSEventStreamEventId).unwrap();

        let res = std::panic::catch_unwind(|| unsafe {
            callback_impl(
                ptr::null(),
                context_ptr,
                1,
                event_paths,
                event_flags,
                event_ids,
            );
        });
        unsafe {
            drop(Box::from_raw(context_ptr as *mut StreamContextInfo));
        }

        assert!(res.is_ok(), "callback_impl should not panic");

        let event = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("expected event")
            .expect("expected Ok(Event)");
        assert!(
            event.kind.is_create(),
            "expected create event, got {event:?}"
        );
    }

    #[test]
    fn translate_flags_ignores_is_file_only_events() {
        assert!(translate_flags(&StreamFlags::IS_FILE, true, None).is_empty());
        assert!(
            translate_flags(
                &(StreamFlags::IS_FILE | StreamFlags::ITEM_CLONED),
                true,
                None
            )
            .is_empty(),
            "type-only clone flags should not produce events"
        );
    }

    #[test]
    fn translate_flags_sets_clone_info_for_file_events() {
        let create = translate_flags(
            &(StreamFlags::ITEM_CREATED | StreamFlags::IS_FILE | StreamFlags::ITEM_CLONED),
            true,
            None,
        );
        assert_eq!(create.len(), 1);
        assert_eq!(create[0].kind, EventKind::Create(CreateKind::File));
        assert_eq!(create[0].info(), Some("is: clone"));

        let modify = translate_flags(
            &(StreamFlags::INODE_META_MOD
                | StreamFlags::ITEM_MODIFIED
                | StreamFlags::IS_FILE
                | StreamFlags::ITEM_CLONED),
            true,
            None,
        );
        assert_eq!(modify.len(), 2);
        assert!(
            modify
                .iter()
                .any(|e| matches!(e.kind, EventKind::Modify(ModifyKind::Metadata(_))))
        );
        assert!(
            modify
                .iter()
                .any(|e| matches!(e.kind, EventKind::Modify(ModifyKind::Data(_))))
        );
        assert!(
            modify.iter().all(|e| e.info() == Some("is: clone")),
            "all events should be annotated as clone-related: {modify:?}"
        );
    }

    #[test]
    fn translate_flags_does_not_override_existing_info() {
        let evs = translate_flags(
            &(StreamFlags::ROOT_CHANGED
                | StreamFlags::ITEM_REMOVED
                | StreamFlags::IS_FILE
                | StreamFlags::ITEM_CLONED),
            true,
            None,
        );
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].info(), Some("root changed"));
    }

    #[test]
    fn does_not_crash_with_empty_path() {
        let mut watcher = FsEventWatcher::new(|_| {}, Config::default()).unwrap();

        let watch_result = watcher.watch(Path::new(""), WatchMode::recursive());
        assert!(
            matches!(
                watch_result,
                Err(Error {
                    kind: ErrorKind::PathNotFound,
                    paths: _
                })
            ),
            "actual: {watch_result:#?}"
        );

        let unwatch_result = watcher.unwatch(Path::new(""));
        assert!(
            matches!(
                unwatch_result,
                Err(Error {
                    kind: ErrorKind::WatchNotFound,
                    paths: _
                })
            ),
            "actual: {unwatch_result:#?}"
        );
    }

    #[test]
    fn create_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        rx.wait_unordered([expected(path).create_file()]);
    }

    #[test]
    fn create_self_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");

        watcher.watch_nonrecursively(&path);

        std::fs::File::create_new(&path).expect("create");

        rx.wait_ordered_exact([expected(&path).create_file()]);
    }

    #[test]
    fn write_file() {
        let tmpdir = testdir();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        let (mut watcher, rx) = watcher();

        watcher.watch_recursively(&tmpdir);

        std::fs::write(&path, b"123").expect("write");

        rx.wait_unordered([expected(&path).modify_data_content()]);
    }

    #[test]
    fn chmod_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        let file = std::fs::File::create_new(&path).expect("create");
        let mut permissions = file.metadata().expect("metadata").permissions();
        permissions.set_readonly(true);

        watcher.watch_recursively(&tmpdir);
        file.set_permissions(permissions).expect("set_permissions");

        rx.wait_unordered([expected(&path).modify_meta_owner()]);
    }

    #[test]
    fn rename_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        let new_path = tmpdir.path().join("renamed");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_unordered([expected(path).rename_any(), expected(new_path).rename_any()]);
    }

    #[test]
    fn rename_self_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_nonrecursively(&path);
        let new_path = tmpdir.path().join("renamed");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_unordered([expected(&path).rename_any()]);

        std::fs::rename(&new_path, &path).expect("rename2");

        rx.wait_unordered([expected(&path).rename_any()]);
    }

    #[test]
    fn rename_self_file_no_track() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch(
            &path,
            WatchMode {
                recursive_mode: RecursiveMode::NonRecursive,
                target_mode: TargetMode::NoTrack,
            },
        );

        let new_path = tmpdir.path().join("renamed");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_unordered([expected(&path).rename_any()]);

        let result = watcher.watcher.watch(
            &path,
            WatchMode {
                recursive_mode: RecursiveMode::NonRecursive,
                target_mode: TargetMode::NoTrack,
            },
        );
        assert!(matches!(
            result,
            Err(Error {
                paths: _,
                kind: ErrorKind::PathNotFound
            })
        ));
    }

    #[test]
    fn delete_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let file = tmpdir.path().join("file");
        std::fs::write(&file, "").expect("write");

        watcher.watch_nonrecursively(&tmpdir);

        std::fs::remove_file(&file).expect("remove");

        rx.wait_unordered([expected(&file).remove_file()]);
    }

    #[test]
    fn delete_self_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let file = tmpdir.path().join("file");
        std::fs::write(&file, "").expect("write");

        watcher.watch_nonrecursively(&file);

        std::fs::remove_file(&file).expect("remove");

        rx.wait_unordered([expected(&file).remove_file()]);

        std::fs::write(&file, "").expect("write");

        rx.wait_ordered_exact([expected(&file).create_file()]);
    }

    #[test]
    fn delete_self_file_no_track() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let file = tmpdir.path().join("file");
        std::fs::write(&file, "").expect("write");

        watcher.watch(
            &file,
            WatchMode {
                recursive_mode: RecursiveMode::NonRecursive,
                target_mode: TargetMode::NoTrack,
            },
        );

        std::fs::remove_file(&file).expect("remove");

        rx.wait_unordered([expected(&file).remove_file()]);

        std::fs::write(&file, "").expect("write");

        // rx.ensure_empty_with_wait(); // TODO: should unwatch
    }

    #[test]
    fn create_write_overwrite() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let overwritten_file = tmpdir.path().join("overwritten_file");
        let overwriting_file = tmpdir.path().join("overwriting_file");
        std::fs::write(&overwritten_file, "123").expect("write1");

        watcher.watch_nonrecursively(&tmpdir);

        std::fs::File::create(&overwriting_file).expect("create");
        std::fs::write(&overwriting_file, "321").expect("write2");
        std::fs::rename(&overwriting_file, &overwritten_file).expect("rename");

        rx.wait_unordered([
            expected(&overwriting_file).create(),
            expected(&overwriting_file).modify_data_content().multiple(),
            expected(&overwriting_file).rename_any(),
            expected(&overwritten_file).rename_any(),
        ]);
    }

    fn assert_track_path_continues_after_recreating_file_in_nested_directory(
        upgrade_from_no_track: bool,
    ) {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();
        let nested_dir = tmpdir.path().join("nested");
        let watched_file = nested_dir.join("watched");
        let moved_file = tmpdir.path().join("moved");
        std::fs::create_dir(&nested_dir).expect("create nested dir");
        std::fs::write(&watched_file, "initial").expect("write watched file");

        watcher.watch_nonrecursively(&tmpdir);
        if upgrade_from_no_track {
            watcher.watch(
                &watched_file,
                WatchMode {
                    recursive_mode: RecursiveMode::NonRecursive,
                    target_mode: TargetMode::NoTrack,
                },
            );
        }
        watcher.watch_nonrecursively(&watched_file);

        std::fs::rename(&watched_file, &moved_file).expect("move watched file");
        std::fs::copy(&moved_file, &watched_file).expect("recreate watched file");
        std::fs::remove_file(&moved_file).expect("remove moved file");

        // Wait until the replacement events are drained before checking the next write.
        for _ in rx.iter() {}

        std::fs::write(&watched_file, "updated").expect("update watched file");
        let received_change = rx.iter().any(|event| {
            event.paths.iter().any(|path| path == &watched_file)
                && matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(ModifyKind::Data(_))
                )
        });

        assert!(
            received_change,
            "expected a change event after recreating the watched file"
        );
    }

    #[test]
    fn track_path_continues_after_recreating_file_in_nested_directory() {
        assert_track_path_continues_after_recreating_file_in_nested_directory(false);
    }

    #[test]
    fn track_path_upgrade_continues_after_recreating_file_in_nested_directory() {
        assert_track_path_continues_after_recreating_file_in_nested_directory(true);
    }

    #[test]
    fn create_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create");

        rx.wait_unordered([expected(&path).create_folder()]);
    }

    #[test]
    fn chmod_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");
        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_readonly(true);

        watcher.watch_recursively(&tmpdir);
        std::fs::set_permissions(&path, permissions).expect("set_permissions");

        rx.wait_unordered([expected(&path).modify_meta_owner()]);
    }

    #[test]
    fn rename_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        let new_path = tmpdir.path().join("new_path");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_ordered([
            expected(&path).rename_any(),
            expected(&new_path).rename_any(),
        ]);
    }

    #[test]
    fn delete_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::remove_dir(&path).expect("remove");

        rx.wait_unordered([expected(path).remove_folder()]);
    }

    #[test]
    fn delete_self_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::remove_dir(&path).expect("remove");

        rx.wait_unordered([expected(&path).remove_folder()]);

        std::fs::create_dir(&path).expect("create_dir2");

        rx.wait_ordered([expected(&path).create_folder()]);
    }

    #[test]
    fn delete_self_dir_no_track() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");

        watcher
            .watcher
            .watch(
                &path,
                WatchMode {
                    recursive_mode: RecursiveMode::Recursive,
                    target_mode: TargetMode::NoTrack,
                },
            )
            .expect("watch");
        std::fs::remove_dir(&path).expect("remove");

        rx.wait_unordered([expected(&path).remove_folder()]);

        std::fs::create_dir(&path).expect("create_dir2");

        // rx.ensure_empty_with_wait(); // TODO: should unwatch
    }

    #[test]
    fn delete_parent_of_watched_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let parent = tmpdir.path().join("parent");
        let child = parent.join("child");
        std::fs::create_dir_all(&child).expect("create_dir_all");

        watcher.watch_recursively(&child);

        std::fs::remove_dir_all(&parent).expect("remove_dir_all");

        rx.wait_unordered([expected(&child).remove_any()]);
    }

    #[test]
    fn rename_parent_of_watched_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let parent = tmpdir.path().join("parent");
        let child = parent.join("child");
        std::fs::create_dir_all(&child).expect("create_dir_all");

        watcher.watch_recursively(&child);

        let new_parent = tmpdir.path().join("renamed_parent");
        std::fs::rename(&parent, &new_parent).expect("rename");

        rx.wait_unordered([expected(&child).remove_any()]);
    }

    #[test]
    fn rename_parent_of_watched_paths_and_back() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let parent = tmpdir.path().join("parent");
        let dir = parent.join("dir");
        let file = parent.join("file");
        std::fs::create_dir_all(&dir).expect("create_dir_all");
        std::fs::write(&file, "1").expect("write");

        watcher.watch_recursively(&dir);
        watcher.watch_nonrecursively(&file);

        let new_parent = tmpdir.path().join("renamed_parent");
        std::fs::rename(&parent, &new_parent).expect("rename away");
        rx.wait_unordered([expected(&dir).remove_any(), expected(&file).remove_any()]);

        std::fs::rename(&new_parent, &parent).expect("rename back");
        rx.wait_unordered([
            expected(&dir).create_folder(),
            expected(&file).create_file(),
        ]);

        std::fs::write(&file, "2").expect("write");
        rx.wait_unordered([expected(&file).modify_data_content()]);
    }

    #[test]
    fn rename_dir_twice() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        let new_path = tmpdir.path().join("new_path");
        let new_path2 = tmpdir.path().join("new_path2");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::rename(&path, &new_path).expect("rename");
        std::fs::rename(&new_path, &new_path2).expect("rename2");

        rx.wait_unordered([
            expected(&path).rename_any(),
            expected(&new_path).rename_any(),
            expected(&new_path2).rename_any(),
        ]);
    }

    #[test]
    fn move_out_of_watched_dir() {
        let tmpdir = testdir();
        let subdir = tmpdir.path().join("subdir");
        let (mut watcher, rx) = watcher();

        let path = subdir.join("entry");
        std::fs::create_dir_all(&subdir).expect("create_dir_all");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&subdir);
        let new_path = tmpdir.path().join("entry");

        std::fs::rename(&path, &new_path).expect("rename");

        rx.wait_unordered([expected(path).rename_any()]);
    }

    #[test]
    #[ignore = "https://github.com/notify-rs/notify/issues/729"]
    fn create_write_write_rename_write_remove() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let file1 = tmpdir.path().join("entry");
        let file2 = tmpdir.path().join("entry2");
        std::fs::File::create_new(&file2).expect("create file2");
        let new_path = tmpdir.path().join("renamed");

        watcher.watch_recursively(&tmpdir);
        std::fs::write(&file1, "123").expect("write 1");
        std::fs::write(&file2, "321").expect("write 2");
        std::fs::rename(&file1, &new_path).expect("rename");
        std::fs::write(&new_path, b"1").expect("write 3");
        std::fs::remove_file(&new_path).expect("remove");

        rx.wait_ordered([
            expected(&file1).create_file(),
            expected(&file1).modify_data_content(),
            expected(&file2).modify_data_content(),
            expected(&file1).rename_any(),
            expected(&new_path).rename_any(),
            expected(&new_path).modify_data_content(),
            expected(&new_path).remove_file(),
        ]);
    }

    #[test]
    fn rename_twice() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        let new_path1 = tmpdir.path().join("renamed1");
        let new_path2 = tmpdir.path().join("renamed2");

        std::fs::rename(&path, &new_path1).expect("rename1");
        std::fs::rename(&new_path1, &new_path2).expect("rename2");

        rx.wait_unordered([
            expected(&path).rename_any(),
            expected(&new_path1).rename_any(),
            expected(&new_path2).rename_any(),
        ]);
    }

    #[test]
    fn set_file_mtime() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        let file = std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);

        file.set_modified(
            std::time::SystemTime::now()
                .checked_sub(Duration::from_secs(60 * 60))
                .expect("time"),
        )
        .expect("set_time");

        rx.wait_unordered([expected(&path).modify_meta_any()]);
    }

    #[test]
    fn write_file_non_recursive_watch() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_nonrecursively(&path);

        std::fs::write(&path, b"123").expect("write");

        rx.wait_unordered([expected(path).modify_data_content()]);
    }

    #[test]
    fn write_to_a_hardlink_pointed_to_the_watched_file_triggers_an_event() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let subdir = tmpdir.path().join("subdir");
        let subdir2 = tmpdir.path().join("subdir2");
        let file = subdir.join("file");
        let hardlink = subdir2.join("hardlink");

        std::fs::create_dir(&subdir).expect("create");
        std::fs::create_dir(&subdir2).expect("create2");
        std::fs::write(&file, "").expect("file");
        std::fs::hard_link(&file, &hardlink).expect("hardlink");

        watcher.watch_nonrecursively(&file);

        std::fs::write(&hardlink, "123123").expect("write to the hard link");

        rx.wait_unordered([expected(file).modify_data_content()]);
    }

    #[test]
    fn recursive_creation() {
        let tmpdir = testdir();
        let nested1 = tmpdir.path().join("1");
        let nested2 = tmpdir.path().join("1/2");
        let nested3 = tmpdir.path().join("1/2/3");
        let nested4 = tmpdir.path().join("1/2/3/4");
        let nested5 = tmpdir.path().join("1/2/3/4/5");
        let nested6 = tmpdir.path().join("1/2/3/4/5/6");
        let nested7 = tmpdir.path().join("1/2/3/4/5/6/7");
        let nested8 = tmpdir.path().join("1/2/3/4/5/6/7/8");
        let nested9 = tmpdir.path().join("1/2/3/4/5/6/7/8/9");

        let (mut watcher, rx) = watcher();

        watcher.watch_recursively(&tmpdir);

        std::fs::create_dir_all(&nested9).expect("create_dir_all");

        rx.wait_ordered([
            expected(&nested1).create_folder(),
            expected(&nested2).create_folder(),
            expected(&nested3).create_folder(),
            expected(&nested4).create_folder(),
            expected(&nested5).create_folder(),
            expected(&nested6).create_folder(),
            expected(&nested7).create_folder(),
            expected(&nested8).create_folder(),
            expected(&nested9).create_folder(),
        ]);
    }

    #[test]
    fn upgrade_to_recursive() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("upgrade");
        let deep = tmpdir.path().join("upgrade/deep");
        let file = tmpdir.path().join("upgrade/deep/file");
        std::fs::create_dir_all(&deep).expect("create_dir");

        watcher.watch_nonrecursively(&path);
        std::fs::File::create_new(&file).expect("create");
        std::fs::remove_file(&file).expect("delete");

        rx.wait_ordered([expected(&deep).modify_data_any().optional()]);

        watcher.watch_recursively(&path);
        std::fs::File::create_new(&file).expect("create");

        rx.wait_ordered([expected(&file).create_file()]);
    }

    // Replaces a test that watched 4097 paths to provoke an `FSEventStreamStart` failure
    // (https://github.com/fsnotify/fsevents/issues/48). That path count is exactly what
    // closes fd 0, so the test corrupted the process it ran in.
    #[test]
    fn refuses_more_paths_than_fsevents_can_carry() {
        let budget = fsevents_path_budget().expect("path budget");
        if budget > 4096 {
            eprintln!("skipping: RLIMIT_NOFILE leaves a budget of {budget} paths");
            return;
        }

        let tmpdir = testdir();
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut watcher = FsEventWatcher::new(tx, Config::default().with_max_fsevent_paths(0))
            .expect("create watcher");

        let mut paths = watcher.paths_mut();
        let depth = (usize::BITS - budget.leading_zeros()).max(1);
        for i in 0..=budget {
            // Use a binary tree so the fork's sibling-path consolidation does not
            // merge the paths before the FSEvents safety check sees them.
            let mut path = tmpdir.path().to_path_buf();
            for bit in (0..depth).rev() {
                path.push(if i & (1 << bit) == 0 { "0" } else { "1" });
            }
            std::fs::create_dir_all(&path).expect("create_dir");
            paths.add(&path, WatchMode::non_recursive()).expect("add");
        }
        let err = paths
            .commit()
            .expect_err("watching more paths than the budget must fail");
        assert!(
            matches!(err.kind, ErrorKind::MaxFilesWatch),
            "expected MaxFilesWatch, got {err:?}"
        );
    }

    #[test]
    fn path_budget_is_shared_across_live_streams() {
        static ACTIVE_PATHS: AtomicUsize = AtomicUsize::new(0);

        let first = FseventsPathReservation::acquire(&ACTIVE_PATHS, 15, 21)
            .expect("first stream must fit within the budget");
        let active_path_count = FseventsPathReservation::acquire(&ACTIVE_PATHS, 15, 21)
            .expect_err("the combined path count must exceed the budget");
        assert_eq!(active_path_count, 15);

        drop(first);

        let second = FseventsPathReservation::acquire(&ACTIVE_PATHS, 15, 21)
            .expect("stopping the first stream must release its paths");
        drop(second);
        assert_eq!(ACTIVE_PATHS.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn rename_then_remove_remove_event_must_be_the_last_one() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        let new_path1 = tmpdir.path().join("renamed1");
        let new_path2 = tmpdir.path().join("renamed2");

        std::fs::rename(&path, &new_path1).expect("rename1");
        std::fs::rename(&new_path1, &new_path2).expect("rename2");

        std::fs::remove_file(&new_path2).expect("remove_file");

        loop {
            let ev = rx.recv();
            if matches!(ev.kind, EventKind::Remove(RemoveKind::File)) {
                assert_eq!(&ev.paths, &[new_path2]);
                break;
            }
        }

        rx.ensure_empty();
    }
}
