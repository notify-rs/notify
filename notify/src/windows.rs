#![allow(missing_docs)]
//! Watcher implementation for Windows' directory management APIs
//!
//! For more information see the [ReadDirectoryChangesW reference][ref].
//!
//! [ref]: https://msdn.microsoft.com/en-us/library/windows/desktop/aa363950(v=vs.85).aspx

use crate::consolidating_path_trie::ConsolidatingPathTrie;
use crate::{
    BoundSender, Config, ErrorKind, PathsMut, Receiver, Sender, TargetMode, WatchMode, bounded,
    unbounded,
};
use crate::{Error, EventHandler, Result, Watcher};
use crate::{WatcherKind, event::*};
use rustc_hash::FxBuildHasher;
use std::alloc;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::os::raw::c_void;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr;
use std::rc::Rc;
use std::slice;
use std::sync::{Arc, Mutex};
use std::thread;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_OPERATION_ABORTED, ERROR_SUCCESS, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED,
    FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_ATTRIBUTES,
    FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SECURITY, FILE_NOTIFY_CHANGE_SIZE,
    FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    ReadDirectoryChangesW,
};
use windows_sys::Win32::System::IO::{CancelIo, OVERLAPPED};
use windows_sys::Win32::System::Threading::{
    CreateSemaphoreW, INFINITE, ReleaseSemaphore, WaitForSingleObjectEx,
};

const BUF_SIZE: u32 = 16384;

fn windows_namespace_prefix_len(path: &[u16]) -> usize {
    let is_separator = |ch: u16| ch == '/' as u16 || ch == '\\' as u16;

    if path.len() >= 4
        && is_separator(path[0])
        && is_separator(path[1])
        && (path[2] == '?' as u16 || path[2] == '.' as u16)
        && is_separator(path[3])
    {
        4
    } else {
        0
    }
}

fn normalize_path_separators(path: PathBuf) -> PathBuf {
    let separator = '\\' as u16;
    let mut encoded_path: Vec<u16> = path.into_os_string().encode_wide().collect();
    let prefix_len = windows_namespace_prefix_len(&encoded_path);

    for ch in encoded_path.iter_mut().skip(prefix_len) {
        if *ch == '/' as u16 || *ch == '\\' as u16 {
            *ch = separator;
        }
    }

    PathBuf::from(OsString::from_wide(&encoded_path))
}

/// The resolved OS-level coverage for a user watch request.
#[derive(Debug, Clone)]
struct ResolvedWatch {
    /// The directory we'd open with `CreateFileW` to receive content events
    /// for this watch, plus whether the user asked for a recursive subtree.
    primary: Option<(PathBuf, bool)>,
    /// Whether the watch also needs an auxiliary watch on the user path's
    /// direct parent so we can detect rename or delete events for the watched
    /// path itself.
    needs_tracked_parent: bool,
}

#[derive(Clone)]
struct ReadData {
    dir: PathBuf, // directory that is being watched
    watches: Rc<RefCell<HashMap<PathBuf, WatchMode, FxBuildHasher>>>,
    /// The ancestors of the tracked paths; a change to one of them changes what the paths below
    /// it can reach.
    ancestors: Rc<RefCell<HashSet<PathBuf, FxBuildHasher>>>,
    complete_sem: HANDLE,
    is_recursive: bool,
}

struct ReadDirectoryRequest {
    event_handler: Arc<Mutex<dyn EventHandler>>,
    buffer: [u8; BUF_SIZE as usize],
    handle: HANDLE,
    data: ReadData,
    action_tx: Sender<Action>,
}

impl ReadDirectoryRequest {
    fn unwatch_raw(&self) {
        let result = self
            .action_tx
            .send(Action::UnwatchRaw(self.data.dir.clone()));
        if let Err(e) = result {
            tracing::error!(?e, "failed to send UnwatchRaw action");
        }
    }
}

enum Action {
    Watch(PathBuf, WatchMode),
    Unwatch(PathBuf),
    UnwatchRaw(PathBuf),
    /// A tracked path, or a directory on the way to one, came or went.
    Changed(PathBuf),
    StageAndCommit(Vec<StagedChange>, BoundSender<Result<()>>),
    Stop,
    Configure(Config, BoundSender<Result<bool>>),
    #[cfg(test)]
    GetWatchHandles(BoundSender<HashSet<PathBuf>>),
}

enum StagedChange {
    Add(PathBuf, WatchMode),
    Remove(PathBuf),
}

struct WatchState {
    dir_handle: HANDLE,
    complete_sem: HANDLE,
}

struct ReadDirectoryChangesServer {
    tx: Sender<Action>,
    rx: Receiver<Action>,
    event_handler: Arc<Mutex<dyn EventHandler>>,
    cmd_tx: Sender<Result<PathBuf>>,
    /// The raw watch request registered by the user, keyed by user path.
    watches: Rc<RefCell<HashMap<PathBuf, WatchMode, FxBuildHasher>>>,
    /// Resolved OS-level coverage for each entry in `watches`, keyed by the
    /// same user path. Resolution needs a `metadata()` call, so it is cached
    /// here rather than recomputed on every rebuild.
    resolved_watches: HashMap<PathBuf, ResolvedWatch, FxBuildHasher>,
    watch_handles: HashMap<PathBuf, (WatchState, /* is_recursive */ bool), FxBuildHasher>,
    /// The ancestors of the tracked paths, shared with the event thread.
    ancestors: Rc<RefCell<HashSet<PathBuf, FxBuildHasher>>>,
    /// The handles opened only to see an ancestor of a tracked path come or go.
    chain_handles: HashSet<PathBuf, FxBuildHasher>,
    wakeup_sem: HANDLE,
}

impl ReadDirectoryChangesServer {
    fn start(
        event_handler: Arc<Mutex<dyn EventHandler>>,
        cmd_tx: Sender<Result<PathBuf>>,
        wakeup_sem: HANDLE,
    ) -> Sender<Action> {
        let (action_tx, action_rx) = unbounded();
        // it is, in fact, ok to send the semaphore across threads
        let sem_temp = wakeup_sem as u64;
        let result = thread::Builder::new()
            .name("notify-rs windows loop".to_string())
            .spawn({
                let tx = action_tx.clone();
                move || {
                    let wakeup_sem = sem_temp as HANDLE;
                    let server = ReadDirectoryChangesServer {
                        tx,
                        rx: action_rx,
                        event_handler,
                        cmd_tx,
                        watches: Rc::new(RefCell::new(HashMap::default())),
                        resolved_watches: HashMap::default(),
                        watch_handles: HashMap::default(),
                        ancestors: Rc::new(RefCell::new(HashSet::default())),
                        chain_handles: HashSet::default(),
                        wakeup_sem,
                    };
                    server.run();
                }
            });
        if let Err(e) = result {
            tracing::error!(?e, "failed to spawn ReadDirectoryChangesWatcher thread");
        }
        action_tx
    }

    fn run(mut self) {
        loop {
            // process all available actions first
            let mut stopped = false;

            while let Ok(action) = self.rx.try_recv() {
                match action {
                    Action::Watch(path, watch_mode) => {
                        let res = self.add_watch(path, watch_mode);
                        let result = self.cmd_tx.send(res);
                        if let Err(e) = result {
                            tracing::error!(?e, "failed to send Watch result");
                        }
                    }
                    Action::Unwatch(path) => self.remove_watch(&path),
                    Action::UnwatchRaw(path) => self.remove_watch_raw(&path),
                    Action::Changed(path) => self.reresolve_below(&path),
                    Action::StageAndCommit(staged, tx) => {
                        let res = self.apply_staged(staged);
                        if let Err(e) = tx.send(res) {
                            tracing::error!(?e, "failed to send StageAndCommit result");
                        }
                    }
                    Action::Stop => {
                        stopped = true;
                        for (ws, _) in self.watch_handles.values() {
                            stop_watch(ws);
                        }
                        break;
                    }
                    Action::Configure(config, tx) => {
                        Self::configure_raw_mode(config, &tx);
                    }
                    #[cfg(test)]
                    Action::GetWatchHandles(tx) => {
                        let handles = self
                            .watch_handles
                            .keys()
                            .filter(|path| !self.chain_handles.contains(*path))
                            .cloned()
                            .collect();
                        tx.send(handles).unwrap();
                    }
                }
            }

            if stopped {
                break;
            }

            unsafe {
                // wait with alertable flag so that the completion routine fires
                WaitForSingleObjectEx(self.wakeup_sem, 100, 1);
            }
        }

        // we have to clean this up, since the watcher may be long gone
        unsafe {
            CloseHandle(self.wakeup_sem);
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn add_watch(&mut self, path: PathBuf, watch_mode: WatchMode) -> Result<PathBuf> {
        self.add_watch_internal(path.clone(), watch_mode)?;
        self.rebuild_watch_handles()?;
        Ok(path)
    }

    /// Register a single user watch: merge `mode` with any existing entry for
    /// `path` (so repeated watches of the same path only ever upgrade
    /// coverage), resolve it, and record both the raw request in `watches` and
    /// the resolved coverage in `resolved_watches`.
    ///
    /// Watch handles are left untouched; the caller drives `rebuild_watch_handles`.
    fn add_watch_internal(&mut self, path: PathBuf, mode: WatchMode) -> Result<()> {
        let merged = match self.watches.borrow().get(&path) {
            Some(existing) => {
                let mut merged = *existing;
                merged.upgrade_with(mode);
                merged
            }
            None => mode,
        };
        let resolved = resolve_user_watch(&path, merged)?;
        self.watches.borrow_mut().insert(path.clone(), merged);
        self.resolved_watches.insert(path, resolved);
        Ok(())
    }

    fn apply_staged(&mut self, staged: Vec<StagedChange>) -> Result<()> {
        tracing::trace!(change_count = staged.len(), "applying staged watch changes");
        let mut first_error: Option<Error> = None;
        for change in staged {
            let res = match change {
                StagedChange::Add(path, mode) => self.add_watch_internal(path, mode),
                StagedChange::Remove(path) => {
                    self.remove_watch_internal(&path);
                    Ok(())
                }
            };
            if let Err(e) = res
                && first_error.is_none()
            {
                first_error = Some(e);
            }
        }
        if let Err(e) = self.rebuild_watch_handles()
            && first_error.is_none()
        {
            first_error = Some(e);
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Converge the open OS-level watch handles with the current set of user
    /// watches.
    fn rebuild_watch_handles(&mut self) -> Result<()> {
        // Drop resolved entries whose user watch is gone.
        // This is needed because the event thread can remove a `NoTrack` entry
        // from it directly (see `handle_event`) without touching `resolved_watches`.
        {
            let watches = self.watches.borrow();
            self.resolved_watches
                .retain(|path, _| watches.contains_key(path));
        }

        // Build `target`: the desired set of OS-level dirs to watch and their
        // recursive flags. It is the consolidated primary dir requests, plus a
        // non-recursive watch on the tracked parent of any watch that needs one.
        let mut trie = ConsolidatingPathTrie::new(true, 0);
        for resolved in self.resolved_watches.values() {
            if let Some((dir, _)) = &resolved.primary {
                trie.insert(dir);
            }
        }
        let mut target: HashMap<PathBuf, bool, FxBuildHasher> = trie
            .values()
            .into_iter()
            .map(|p| {
                let recursive = compute_recursive_flag(&p, &self.resolved_watches);
                (p, recursive)
            })
            .collect();
        for (path, resolved) in &self.resolved_watches {
            if resolved.needs_tracked_parent
                && let Some(parent) = path.parent()
                && !target.contains_key(parent)
                && parent.is_dir()
            {
                target.insert(parent.to_path_buf(), false);
            }
        }

        // The ancestors of the tracked paths, so that a directory moved away or deleted above a
        // tracked path is seen, and a missing one is seen once it appears. The ones below a
        // recursive watch are seen through it.
        let mut ancestors: HashSet<PathBuf, FxBuildHasher> = HashSet::default();
        for (path, mode) in self.watches.borrow().iter() {
            if mode.target_mode == TargetMode::TrackPath {
                ancestors.extend(path.ancestors().skip(1).map(Path::to_path_buf));
            }
        }
        self.chain_handles.clear();
        for ancestor in &ancestors {
            let covered = target.iter().any(|(dir, recursive)| {
                dir == ancestor || (*recursive && ancestor.starts_with(dir))
            });
            if covered || !ancestor.is_dir() {
                continue;
            }
            target.insert(ancestor.clone(), false);
            self.chain_handles.insert(ancestor.clone());
        }
        *self.ancestors.borrow_mut() = ancestors;
        tracing::trace!(desired = ?target, "rebuilding watch handles");

        let to_remove: Vec<PathBuf> = self
            .watch_handles
            .iter()
            .filter(|(p, (_, is_rec))| target.get(*p).is_none_or(|t| t != is_rec))
            .map(|(p, _)| p.clone())
            .collect();
        if !to_remove.is_empty() {
            tracing::trace!(
                ?to_remove,
                "closing watch handles that are no longer needed"
            );
        }
        for p in to_remove {
            if let Some((ws, _)) = self.watch_handles.remove(&p) {
                stop_watch(&ws);
            }
        }

        let to_open: Vec<(PathBuf, bool)> = target
            .into_iter()
            .filter(|(p, _)| !self.watch_handles.contains_key(p))
            .collect();
        let mut first_error: Option<Error> = None;
        for (path, is_recursive) in to_open {
            if let Err(e) = self.add_watch_raw(path, is_recursive, false)
                && first_error.is_none()
            {
                first_error = Some(e);
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn add_watch_raw(
        &mut self,
        path: PathBuf,
        is_recursive: bool,
        watching_file: bool,
    ) -> Result<()> {
        if let Some((ws, was_recursive)) = self.watch_handles.get(&path) {
            let need_upgrade_to_recursive = !*was_recursive && is_recursive;
            if !need_upgrade_to_recursive {
                tracing::trace!(
                    "watch handle already exists and no need to upgrade: {}",
                    path.display()
                );
                return Ok(());
            }
            tracing::trace!("upgrading watch handle to recursive: {}", path.display());
            stop_watch(ws);
        }

        let encoded_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle;
        unsafe {
            handle = CreateFileW(
                encoded_path.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_DELETE | FILE_SHARE_WRITE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            );

            if handle == INVALID_HANDLE_VALUE {
                return Err(if watching_file {
                    Error::generic(
                        "You attempted to watch a single file, but parent \
                         directory could not be opened.",
                    )
                    .add_path(path)
                } else {
                    // TODO: Call GetLastError for better error info?
                    Error::path_not_found().add_path(path)
                });
            }
        }
        // every watcher gets its own semaphore to signal completion
        let semaphore = unsafe { CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
        if semaphore.is_null() || semaphore == INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(handle);
            }
            return Err(Error::generic("Failed to create semaphore for watch.").add_path(path));
        }
        let rd = ReadData {
            dir: path.clone(),
            watches: Rc::clone(&self.watches),
            ancestors: Rc::clone(&self.ancestors),
            complete_sem: semaphore,
            is_recursive,
        };
        let ws = WatchState {
            dir_handle: handle,
            complete_sem: semaphore,
        };
        self.watch_handles.insert(path, (ws, is_recursive));
        start_read(
            &rd,
            Arc::clone(&self.event_handler),
            handle,
            self.tx.clone(),
        );
        Ok(())
    }

    /// Remove a single user watch from `watches` and `resolved_watches`,
    /// returning whether an entry was present. Watch handles are left
    /// untouched; the caller drives `rebuild_watch_handles`.
    fn remove_watch_internal(&mut self, path: &Path) -> bool {
        let removed = self.watches.borrow_mut().remove(path).is_some();
        self.resolved_watches.remove(path);
        removed
    }

    #[tracing::instrument(level = "trace", skip(self))]
    fn remove_watch(&mut self, path: &Path) {
        if self.remove_watch_internal(path)
            && let Err(e) = self.rebuild_watch_handles()
        {
            tracing::error!(?e, "failed to rebuild watch handles after remove_watch");
        }
    }

    /// Drop a watch handle out-of-band when the OS reports that the directory is
    /// gone (or some other error invalidated it). The handle entry is purged
    /// without consulting `self.watches`, since the corresponding user watch
    /// may still be present and will be re-opened on the next rebuild.
    #[tracing::instrument(level = "trace", skip(self))]
    fn remove_watch_raw(&mut self, path: &Path) {
        if let Some((ws, _)) = self.watch_handles.remove(path) {
            stop_watch(&ws);
        }
    }

    fn configure_raw_mode(_config: Config, tx: &BoundSender<Result<bool>>) {
        tx.send(Ok(false))
            .expect("configuration channel disconnect");
    }

    /// `path` came or went: the tracked paths at or below it may be reachable or out of reach
    /// now. Re-resolve them, report the ones whose presence changed, and converge the handles.
    /// The event on `path` itself was reported by the handle that saw it.
    fn reresolve_below(&mut self, path: &Path) {
        let roots: Vec<(PathBuf, WatchMode)> = self
            .watches
            .borrow()
            .iter()
            .filter(|(root, mode)| {
                mode.target_mode == TargetMode::TrackPath && root.starts_with(path)
            })
            .map(|(root, mode)| (root.clone(), *mode))
            .collect();
        for (root, mode) in roots {
            let was_present = self
                .resolved_watches
                .get(&root)
                .is_some_and(|resolved| resolved.primary.is_some());
            let resolved = match resolve_user_watch(&root, mode) {
                Ok(resolved) => resolved,
                Err(e) => {
                    tracing::debug!(?e, "cannot resolve tracked path: {}", root.display());
                    continue;
                }
            };
            let is_present = resolved.primary.is_some();
            self.resolved_watches.insert(root.clone(), resolved);
            if root.as_path() == path || was_present == is_present {
                continue;
            }
            let kind = if is_present {
                EventKind::Create(if root.is_dir() {
                    CreateKind::Folder
                } else {
                    CreateKind::File
                })
            } else {
                EventKind::Remove(RemoveKind::Any)
            };
            if let Ok(mut handler) = self.event_handler.lock() {
                handler.handle_event(Ok(Event::new(kind).add_path(root)));
            }
        }
        if let Err(e) = self.rebuild_watch_handles() {
            tracing::error!(
                ?e,
                "failed to rebuild watch handles after a tracked path changed"
            );
        }
    }
}

/// Resolve a user-supplied watch path + mode into a [`ResolvedWatch`] describing
/// which OS-level directories we'd want to watch.
fn resolve_user_watch(path: &Path, mode: WatchMode) -> Result<ResolvedWatch> {
    let is_track_path = mode.target_mode == TargetMode::TrackPath;

    // Note: reading metadata on a directory triggers a modify event
    match path.metadata().map_err(Error::io_watch) {
        Ok(meta) => {
            if meta.is_dir() {
                Ok(ResolvedWatch {
                    primary: Some((path.to_path_buf(), mode.recursive_mode.is_recursive())),
                    needs_tracked_parent: is_track_path,
                })
            } else if meta.is_file() {
                let parent = path.parent().unwrap_or(path).to_path_buf();
                Ok(ResolvedWatch {
                    primary: Some((parent, mode.recursive_mode.is_recursive())),
                    // For files we always have to watch the parent directory anyway,
                    // so the "tracked parent" rename-detection requirement is
                    // already covered by `primary`; no separate entry needed.
                    needs_tracked_parent: false,
                })
            } else {
                Err(
                    Error::generic("Input watch path is neither a file nor a directory.")
                        .add_path(path.to_path_buf()),
                )
            }
        }
        Err(err) => {
            // For TrackPath we keep the watch alive and rely on the parent dir
            // to tell us when something appears at `path`.
            if is_track_path && matches!(err.kind, ErrorKind::PathNotFound) {
                Ok(ResolvedWatch {
                    primary: None,
                    needs_tracked_parent: true,
                })
            } else {
                Err(err)
            }
        }
    }
}

/// Decide whether a consolidated OS-level watch on `target_path` must be opened
/// with `bWatchSubtree=1`.
fn compute_recursive_flag(
    target_path: &Path,
    resolved_watches: &HashMap<PathBuf, ResolvedWatch, FxBuildHasher>,
) -> bool {
    resolved_watches
        .values()
        .filter_map(|resolved| resolved.primary.as_ref())
        .any(|(dir, is_rec)| (dir == target_path && *is_rec) || dir.starts_with(target_path))
}

/// Returns `true` if an event on `event_path` is covered by some user-registered
/// watch.
fn is_event_covered(
    watches: &HashMap<PathBuf, WatchMode, FxBuildHasher>,
    event_path: &Path,
) -> bool {
    event_path.ancestors().enumerate().any(|(depth, ancestor)| {
        watches
            .get(ancestor)
            .is_some_and(|mode| depth <= 1 || mode.recursive_mode.is_recursive())
    })
}

fn stop_watch(ws: &WatchState) {
    tracing::trace!("removing ReadDirectoryChangesW watch");
    unsafe {
        let cio = CancelIo(ws.dir_handle);
        let ch = CloseHandle(ws.dir_handle);
        // have to wait for it, otherwise we leak the memory allocated for there read request
        if cio != 0 && ch != 0 {
            while WaitForSingleObjectEx(ws.complete_sem, INFINITE, 1) != WAIT_OBJECT_0 {
                // drain the apc queue, fix for https://github.com/notify-rs/notify/issues/287#issuecomment-801465550
            }
        }
        CloseHandle(ws.complete_sem);
    }
}

fn start_read(
    rd: &ReadData,
    event_handler: Arc<Mutex<dyn EventHandler>>,
    handle: HANDLE,
    action_tx: Sender<Action>,
) {
    tracing::trace!("starting ReadDirectoryChangesW watch: {}", rd.dir.display());

    let request = Box::new(ReadDirectoryRequest {
        event_handler,
        handle,
        buffer: [0u8; BUF_SIZE as usize],
        data: rd.clone(),
        action_tx,
    });

    let flags = FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_ATTRIBUTES
        | FILE_NOTIFY_CHANGE_SIZE
        | FILE_NOTIFY_CHANGE_LAST_WRITE
        | FILE_NOTIFY_CHANGE_CREATION
        | FILE_NOTIFY_CHANGE_SECURITY;

    let monitor_subdir = i32::from(request.data.is_recursive);

    unsafe {
        #[expect(clippy::cast_ptr_alignment)]
        let overlapped =
            alloc::alloc_zeroed(alloc::Layout::new::<OVERLAPPED>()).cast::<OVERLAPPED>();
        // When using callback based async requests, we are allowed to use the hEvent member
        // for our own purposes

        let request = Box::leak(request);
        (*overlapped).hEvent = std::ptr::from_mut(request).cast();

        // This is using an asynchronous call with a completion routine for receiving notifications
        // An I/O completion port would probably be more performant
        let ret = ReadDirectoryChangesW(
            handle,
            request.buffer.as_mut_ptr().cast::<c_void>(),
            BUF_SIZE,
            monitor_subdir,
            flags,
            std::ptr::from_mut::<u32>(&mut 0u32), // not used for async reqs
            overlapped,
            Some(handle_event),
        );

        if ret == 0 {
            // error reading. retransmute request memory to allow drop.
            // Because of the error, ownership of the `overlapped` alloc was not passed
            // over to `ReadDirectoryChangesW`.
            // So we can claim ownership back.
            let _overlapped = Box::from_raw(overlapped);
            let request = Box::from_raw(request);
            ReleaseSemaphore(request.data.complete_sem, 1, ptr::null_mut());
        }
    }
}

#[expect(clippy::too_many_lines)]
unsafe extern "system" fn handle_event(
    error_code: u32,
    _bytes_written: u32,
    overlapped: *mut OVERLAPPED,
) {
    let overlapped: Box<OVERLAPPED> = unsafe { Box::from_raw(overlapped) };
    let request: Box<ReadDirectoryRequest> = unsafe { Box::from_raw(overlapped.hEvent.cast()) };

    let release_semaphore =
        || unsafe { ReleaseSemaphore(request.data.complete_sem, 1, ptr::null_mut()) };

    fn emit_event(event_handler: &Mutex<dyn EventHandler>, res: Result<Event>) {
        if let Ok(mut guard) = event_handler.lock() {
            let f: &mut dyn EventHandler = &mut *guard;
            f.handle_event(res);
        }
    }
    let event_handler = |res| emit_event(&request.event_handler, res);

    if error_code != ERROR_SUCCESS {
        tracing::trace!(
            path = ?request.data.dir,
            is_recursive = request.data.is_recursive,
            "ReadDirectoryChangesW handle_event called with error code {error_code}",
        );
    }

    match error_code {
        ERROR_OPERATION_ABORTED => {
            // received when dir is unwatched or watcher is shutdown; return and let overlapped/request get drop-cleaned
            release_semaphore();
            return;
        }
        ERROR_ACCESS_DENIED => {
            let dir = request.data.dir.clone();
            // This could happen when the watched directory is deleted or trashed, first check if it's the case.
            // If so, unwatch the directory and return, otherwise, continue to handle the event.
            if !dir.exists() {
                tracing::debug!(
                    path = ?request.data.dir,
                    is_recursive = request.data.is_recursive,
                    "ReadDirectoryChangesW handle_event: ERROR_ACCESS_DENIED event and directory no longer exists",
                );
                if request
                    .data
                    .watches
                    .borrow()
                    .get(&dir)
                    .is_some_and(|mode| mode.target_mode == TargetMode::NoTrack)
                {
                    let ev = Event::new(EventKind::Remove(RemoveKind::Any)).add_path(dir);
                    event_handler(Ok(ev));
                }
                request.unwatch_raw();
                release_semaphore();
                return;
            }
        }
        ERROR_SUCCESS => {
            // Success, continue to handle the event
        }
        _ => {
            // Some unidentified error occurred, log and unwatch the directory, then return.
            tracing::error!(
                "unknown error in ReadDirectoryChangesW for directory {}: {}",
                request.data.dir.display(),
                error_code
            );
            request.unwatch_raw();
            release_semaphore();
            return;
        }
    }

    // Get the next request queued up as soon as possible
    let action_tx = request.action_tx.clone();
    start_read(
        &request.data,
        Arc::clone(&request.event_handler),
        request.handle,
        request.action_tx,
    );

    let mut remove_paths = vec![];

    // The FILE_NOTIFY_INFORMATION struct has a variable length due to the variable length
    // string as its last member. Each struct contains an offset for getting the next entry in
    // the buffer.
    let mut cur_offset: *const u8 = request.buffer.as_ptr();
    // In Wine, FILE_NOTIFY_INFORMATION structs are packed placed in the buffer;
    // they are aligned to 16bit (WCHAR) boundary instead of 32bit required by FILE_NOTIFY_INFORMATION.
    // Hence, we need to use `read_unaligned` here to avoid UB.
    let mut cur_entry =
        unsafe { ptr::read_unaligned(cur_offset.cast::<FILE_NOTIFY_INFORMATION>()) };
    loop {
        // filename length is size in bytes, so / 2
        let len = cur_entry.FileNameLength as usize / 2;
        let encoded_path: &[u16] = unsafe {
            slice::from_raw_parts(
                cur_offset
                    .add(std::mem::offset_of!(FILE_NOTIFY_INFORMATION, FileName))
                    .cast(),
                len,
            )
        };
        // prepend root to get a full path
        let path = normalize_path_separators(
            request
                .data
                .dir
                .join(PathBuf::from(OsString::from_wide(encoded_path))),
        );

        // A tracked path, or a directory on the way to one, came or went: the server re-resolves
        // what can be reached now.
        let structural = matches!(
            cur_entry.Action,
            FILE_ACTION_ADDED
                | FILE_ACTION_REMOVED
                | FILE_ACTION_RENAMED_OLD_NAME
                | FILE_ACTION_RENAMED_NEW_NAME
        );
        if structural
            && (request.data.ancestors.borrow().contains(&path)
                || request
                    .data
                    .watches
                    .borrow()
                    .get(&path)
                    .is_some_and(|mode| mode.target_mode == TargetMode::TrackPath))
            && let Err(e) = action_tx.send(Action::Changed(path.clone()))
        {
            tracing::error!(?e, "failed to send Changed action");
        }

        let skip = !is_event_covered(&request.data.watches.borrow(), &path);

        tracing::trace!(
            handle_path = ?request.data.dir,
            is_recursive = request.data.is_recursive,
            ?path,
            skip,
            action = cur_entry.Action,
            "ReadDirectoryChangesW handle_event called",
        );

        if !skip {
            let newe = Event::new(EventKind::Any).add_path(path.clone());

            match cur_entry.Action {
                FILE_ACTION_RENAMED_OLD_NAME => {
                    remove_paths.push(path.clone());
                    let kind = EventKind::Modify(ModifyKind::Name(RenameMode::From));
                    let ev = newe.set_kind(kind);
                    event_handler(Ok(ev));
                }
                FILE_ACTION_RENAMED_NEW_NAME => {
                    let kind = EventKind::Modify(ModifyKind::Name(RenameMode::To));
                    let ev = newe.set_kind(kind);
                    event_handler(Ok(ev));
                }
                FILE_ACTION_ADDED => {
                    let kind = EventKind::Create(CreateKind::Any);
                    let ev = newe.set_kind(kind);
                    event_handler(Ok(ev));
                }
                FILE_ACTION_REMOVED => {
                    remove_paths.push(path.clone());
                    let kind = EventKind::Remove(RemoveKind::Any);
                    let ev = newe.set_kind(kind);
                    event_handler(Ok(ev));
                }
                FILE_ACTION_MODIFIED => {
                    let kind = EventKind::Modify(ModifyKind::Any);
                    let ev = newe.set_kind(kind);
                    event_handler(Ok(ev));
                }
                _ => (),
            }
        }

        if cur_entry.NextEntryOffset == 0 {
            break;
        }
        cur_offset = unsafe { cur_offset.add(cur_entry.NextEntryOffset as usize) };
        cur_entry = unsafe { ptr::read_unaligned(cur_offset.cast::<FILE_NOTIFY_INFORMATION>()) };
    }

    tracing::trace!(
        ?remove_paths,
        "processing ReadDirectoryChangesW watch changes",
    );

    for path in remove_paths {
        let is_no_track = {
            request
                .data
                .watches
                .borrow()
                .get(&path)
                .is_some_and(|mode| mode.target_mode == TargetMode::NoTrack)
        };
        if is_no_track {
            request.data.watches.borrow_mut().remove(&path);
        }
    }
}

/// Watcher implementation based on ReadDirectoryChanges
#[derive(Debug)]
pub struct ReadDirectoryChangesWatcher {
    tx: Sender<Action>,
    cmd_rx: Receiver<Result<PathBuf>>,
    wakeup_sem: HANDLE,
}

impl ReadDirectoryChangesWatcher {
    pub fn create(
        event_handler: Arc<Mutex<dyn EventHandler>>,
    ) -> Result<ReadDirectoryChangesWatcher> {
        let (cmd_tx, cmd_rx) = unbounded();

        let wakeup_sem = unsafe { CreateSemaphoreW(ptr::null_mut(), 0, 1, ptr::null_mut()) };
        if wakeup_sem.is_null() || wakeup_sem == INVALID_HANDLE_VALUE {
            return Err(Error::generic("Failed to create wakeup semaphore."));
        }

        let action_tx = ReadDirectoryChangesServer::start(event_handler, cmd_tx, wakeup_sem);

        Ok(ReadDirectoryChangesWatcher {
            tx: action_tx,
            cmd_rx,
            wakeup_sem,
        })
    }

    fn wakeup_server(&mut self) {
        // breaks the server out of its wait state.  right now this is really just an optimization,
        // so that if you add a watch you don't block for 100ms in watch() while the
        // server sleeps.
        unsafe {
            ReleaseSemaphore(self.wakeup_sem, 1, ptr::null_mut());
        }
    }

    fn send_action_require_ack(&mut self, action: Action, pb: &Path) -> Result<()> {
        self.tx
            .send(action)
            .map_err(|_| Error::generic("Error sending to internal channel"))?;

        // wake 'em up, we don't want to wait around for the ack
        self.wakeup_server();

        let ack_pb = self
            .cmd_rx
            .recv()
            .map_err(|_| Error::generic("Error receiving from command channel"))??;

        if pb == ack_pb.as_path() {
            Ok(())
        } else {
            Err(Error::generic(&format!(
                "Expected ack for {} but got \
                 ack for {}",
                pb.display(),
                ack_pb.display()
            )))
        }
    }

    fn watch_inner(&mut self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        let pb = if path.is_absolute() {
            path.to_owned()
        } else {
            let p = env::current_dir().map_err(Error::io)?;
            p.join(path)
        };
        self.send_action_require_ack(Action::Watch(pb.clone(), watch_mode), &pb)
    }

    fn unwatch_inner(&mut self, path: &Path) -> Result<()> {
        let pb = if path.is_absolute() {
            path.to_owned()
        } else {
            let p = env::current_dir().map_err(Error::io)?;
            p.join(path)
        };
        let res = self
            .tx
            .send(Action::Unwatch(pb))
            .map_err(|_| Error::generic("Error sending to internal channel"));
        self.wakeup_server();
        res
    }
}

/// Batched [`PathsMut`] implementation for the Windows backend.
///
/// `add` and `remove` only stage the change in a local `Vec`; nothing crosses
/// the channel until `commit`, at which point the server applies the staged
/// changes in order and runs consolidation once. On error the first error
/// is propagated and the remaining staged operations are skipped at staging
/// time, but any operations that did make it into `self.watches` before the
/// failure remain applied.
struct WindowsPathsMut<'a> {
    watcher: &'a mut ReadDirectoryChangesWatcher,
    staged: Vec<StagedChange>,
}

impl WindowsPathsMut<'_> {
    fn absolutize(path: &Path) -> Result<PathBuf> {
        if path.is_absolute() {
            Ok(path.to_owned())
        } else {
            let cwd = env::current_dir().map_err(Error::io)?;
            Ok(cwd.join(path))
        }
    }
}

impl PathsMut for WindowsPathsMut<'_> {
    #[tracing::instrument(level = "debug", skip(self))]
    fn add(&mut self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        let pb = Self::absolutize(path)?;
        self.staged.push(StagedChange::Add(pb, watch_mode));
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn remove(&mut self, path: &Path) -> Result<()> {
        let pb = Self::absolutize(path)?;
        self.staged.push(StagedChange::Remove(pb));
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn commit(self: Box<Self>) -> Result<()> {
        let WindowsPathsMut { watcher, staged } = *self;
        if staged.is_empty() {
            return Ok(());
        }
        let (tx, rx) = bounded(1);
        watcher
            .tx
            .send(Action::StageAndCommit(staged, tx))
            .map_err(|_| Error::generic("Error sending to internal channel"))?;
        watcher.wakeup_server();
        rx.recv()
            .map_err(|_| Error::generic("Error receiving from commit channel"))?
    }
}

impl Watcher for ReadDirectoryChangesWatcher {
    #[tracing::instrument(level = "debug", skip(event_handler))]
    #[expect(clippy::used_underscore_binding)]
    fn new<F: EventHandler>(event_handler: F, _config: Config) -> Result<Self> {
        let event_handler = Arc::new(Mutex::new(event_handler));
        Self::create(event_handler)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn watch(&mut self, path: &Path, watch_mode: WatchMode) -> Result<()> {
        self.watch_inner(path, watch_mode)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.unwatch_inner(path)
    }

    fn paths_mut<'me>(&'me mut self) -> Box<dyn PathsMut + 'me> {
        Box::new(WindowsPathsMut {
            watcher: self,
            staged: Vec::new(),
        })
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn configure(&mut self, config: Config) -> Result<bool> {
        let (tx, rx) = bounded(1);
        self.tx.send(Action::Configure(config, tx))?;
        rx.recv()?
    }

    fn kind() -> crate::WatcherKind {
        WatcherKind::ReadDirectoryChangesWatcher
    }

    #[cfg(test)]
    fn get_watch_handles(&self) -> HashSet<PathBuf> {
        let (tx, rx) = bounded(1);
        self.tx.send(Action::GetWatchHandles(tx)).unwrap();
        rx.recv().unwrap()
    }
}

impl Drop for ReadDirectoryChangesWatcher {
    fn drop(&mut self) {
        let result = self.tx.send(Action::Stop);
        if let Err(e) = result {
            tracing::error!(?e, "failed to send Stop action");
        }
        // better wake it up
        self.wakeup_server();
    }
}

// `ReadDirectoryChangesWatcher` is not Send/Sync because of the semaphore Handle.
// As said elsewhere it's perfectly safe to send it across threads.
unsafe impl Send for ReadDirectoryChangesWatcher {}
// Because all public methods are `&mut self` it's also perfectly safe to share references.
unsafe impl Sync for ReadDirectoryChangesWatcher {}

#[cfg(test)]
pub mod tests {
    use crate::{
        Error, ErrorKind, ReadDirectoryChangesWatcher, RecursiveMode, TargetMode, WatchMode,
        Watcher, event::EventKind, test::*, windows::normalize_path_separators,
    };

    use std::{
        collections::HashSet, ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf,
        time::Duration,
    };

    fn watcher() -> (TestWatcher<ReadDirectoryChangesWatcher>, Receiver) {
        channel()
    }

    #[test]
    fn trash_dir() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = testdir();
        let child_dir = dir.path().join("child");
        std::fs::create_dir(&child_dir)?;

        let mut watcher = crate::recommended_watcher(|_| {
            // Do something with the event
        })?;
        watcher.watch(&child_dir, WatchMode::non_recursive())?;
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([dir.to_path_buf(), child_dir.clone()])
        );

        trash::delete(&child_dir)?;

        watcher.watch(dir.path(), WatchMode::non_recursive())?;
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([dir.parent_path_buf(), dir.to_path_buf()])
        );

        Ok(())
    }

    #[test]
    fn watcher_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<ReadDirectoryChangesWatcher>();
    }

    #[test]
    fn normalize_joined_event_path_for_posix_watch_path() {
        let dir = PathBuf::from("G:/Feature");
        let raw_event_name: Vec<u16> = "22.mp4".encode_utf16().collect();
        let relative = PathBuf::from(OsString::from_wide(&raw_event_name));
        let path = normalize_path_separators(dir.join(relative));

        assert_eq!(path, PathBuf::from(r"G:\Feature\22.mp4"));
    }

    #[test]
    fn normalize_path_separators_keeps_windows_namespace_prefix() {
        let path = PathBuf::from(r"\\?\C:/very/long/file");
        let normalized = normalize_path_separators(path);
        assert_eq!(normalized, PathBuf::from(r"\\?\C:\very\long\file"));
    }

    #[test]
    fn create_file_normalized() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let tmpdir_without_prefix =
            PathBuf::from(tmpdir.path().to_str().unwrap().replace("\\\\?\\", ""));
        let tmpdir_normalized =
            PathBuf::from(tmpdir_without_prefix.to_str().unwrap().replace('\\', "/"));
        watcher.watch_recursively(&tmpdir_normalized);

        let path = tmpdir_without_prefix.join("entry");
        std::fs::File::create_new(&path).expect("create");

        let event = rx.recv();
        assert_eq!(event.paths.len(), 1);
        assert_eq!(event.paths[0], path);
        assert_eq!(event.paths[0].to_str().unwrap(), path.to_str().unwrap());
    }

    #[test]
    fn create_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        rx.wait_ordered_exact([expected(&path).create_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn create_self_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");

        watcher.watch_nonrecursively(&path);

        std::fs::File::create_new(&path).expect("create");

        rx.wait_ordered_exact([expected(&path).create_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn create_self_file_no_track() {
        let tmpdir = testdir();
        let (mut watcher, _) = watcher();

        let path = tmpdir.path().join("entry");

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
    fn create_self_file_nested() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry/nested");

        watcher.watch_nonrecursively(&path);
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));

        std::fs::create_dir_all(path.parent().unwrap()).expect("create");
        std::fs::File::create_new(&path).expect("create");

        // Reported by the parent once it is watched, or by the watcher if the file is there by
        // then; the kind differs.
        rx.wait_ordered([expected(&path).create()]);
        assert!(
            watcher
                .get_watch_handles()
                .is_superset(&HashSet::from([tmpdir.path().join("entry")]))
        );
    }

    #[test]
    fn track_path_reports_roots_when_an_ancestor_moves_away_and_back() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let lib = tmpdir.path().join("lib");
        let a = lib.join("a.js");
        let b = lib.join("sub").join("b.js");
        let moved = tmpdir.path().join("moved");
        std::fs::create_dir_all(lib.join("sub")).expect("create_dir_all");
        std::fs::write(&a, "1").expect("write");
        std::fs::write(&b, "1").expect("write");

        watcher.watch_nonrecursively(&a);
        watcher.watch_nonrecursively(&b);

        std::fs::rename(&lib, &moved).expect("rename away");
        rx.wait_unordered([expected(&a).remove_any(), expected(&b).remove_any()]);

        std::fs::rename(&moved, &lib).expect("rename back");
        rx.wait_unordered([expected(&a).create_file(), expected(&b).create_file()]);

        std::fs::write(&a, "2").expect("write");
        rx.wait_unordered([expected(&a).modify_any()]);
    }

    #[test]
    fn track_path_watches_a_directory_root_once_it_appears() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let dir = tmpdir.path().join("dir");
        watcher.watch_recursively(&dir);

        std::fs::create_dir(&dir).expect("create_dir");
        rx.wait_unordered([expected(&dir).create()]);

        std::fs::File::create_new(dir.join("file")).expect("create");
        rx.wait_unordered([expected(dir.join("file")).create()]);
    }

    #[test]
    fn write_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_recursively(&tmpdir);
        std::fs::write(&path, b"123").expect("write");

        rx.wait_ordered_exact([expected(&path).modify_any().multiple()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([expected(&path).modify_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([
            expected(&path).rename_from(),
            expected(&new_path).rename_to(),
            expected(tmpdir.path()).modify_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([expected(&path).rename_from()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );

        std::fs::rename(&new_path, &path).expect("rename2");

        rx.wait_ordered_exact([expected(&path).rename_to(), expected(&path).modify_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([expected(&path).rename_from()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );

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

        rx.wait_ordered_exact([expected(&file).remove_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn delete_self_file() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let file = tmpdir.path().join("file");
        std::fs::write(&file, "").expect("write");

        watcher.watch_nonrecursively(&file);

        std::fs::remove_file(&file).expect("remove");

        rx.wait_ordered_exact([expected(&file).remove_any()]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );

        std::fs::write(&file, "").expect("write");

        rx.wait_ordered_exact([expected(&file).create_any()]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([expected(&file).remove_any()]);
        // TODO: can remove from watch, but currently not removed
        // assert_eq!(watcher.get_watch_handles(), HashSet::from([]));

        // std::fs::write(&file, "").expect("write");

        // rx.ensure_empty_with_wait();
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

        rx.wait_ordered_exact([
            expected(&overwriting_file).create_any(),
            expected(tmpdir.path()).modify_any(),
            expected(&overwriting_file).modify_any().multiple(),
            expected(&overwritten_file).remove_any(),
            expected(tmpdir.path()).modify_any().optional(),
            expected(&overwriting_file).rename_from(),
            expected(&overwritten_file).rename_to(),
            expected(tmpdir.path()).modify_any().optional(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn create_self_write_overwrite() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let overwritten_file = tmpdir.path().join("overwritten_file");
        let overwriting_file = tmpdir.path().join("overwriting_file");
        std::fs::write(&overwritten_file, "123").expect("write1");

        watcher.watch_nonrecursively(&overwritten_file);

        std::fs::File::create(&overwriting_file).expect("create");
        std::fs::write(&overwriting_file, "321").expect("write2");
        std::fs::rename(&overwriting_file, &overwritten_file).expect("rename");

        rx.wait_ordered_exact([
            expected(&overwritten_file).remove_any(),
            expected(&overwritten_file).rename_to(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );
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
                && matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_))
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
    fn create_self_write_overwrite_no_track() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        let overwritten_file = tmpdir.path().join("overwritten_file");
        let overwriting_file = tmpdir.path().join("overwriting_file");
        std::fs::write(&overwritten_file, "123").expect("write1");

        watcher.watch(
            &overwritten_file,
            WatchMode {
                recursive_mode: RecursiveMode::NonRecursive,
                target_mode: TargetMode::NoTrack,
            },
        );

        std::fs::File::create(&overwriting_file).expect("create");
        std::fs::write(&overwriting_file, "321").expect("write2");
        std::fs::rename(&overwriting_file, &overwritten_file).expect("rename");

        rx.wait_ordered_exact([expected(&overwritten_file).remove_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()]) // TODO: can remove from watch, but currently not removed
        );
    }

    #[test]
    fn create_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();
        watcher.watch_recursively(&tmpdir);

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create");

        rx.wait_ordered_exact([expected(&path).create_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([expected(&path).modify_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([
            expected(&path).rename_from(),
            expected(&new_path).rename_to(),
            expected(tmpdir.path()).modify_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn delete_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&tmpdir);
        std::fs::remove_dir(&path).expect("remove");

        rx.wait_ordered_exact([expected(&path).remove_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn delete_self_dir() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::create_dir(&path).expect("create_dir");

        watcher.watch_recursively(&path);
        std::fs::remove_dir(&path).expect("remove");

        rx.wait_ordered_exact([expected(&path).remove_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );

        std::fs::create_dir(&path).expect("create_dir2");

        rx.wait_ordered_exact([expected(&path).create_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([expected(&path).remove_any()])
            .ensure_no_tail();
        assert_eq!(watcher.get_watch_handles(), HashSet::from([]));

        std::fs::create_dir(&path).expect("create_dir2");

        rx.ensure_empty_with_wait();
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

        rx.wait_ordered_exact([
            expected(&path).rename_from(),
            expected(&new_path).rename_to(),
            expected(tmpdir.path()).modify_any(),
            expected(&new_path).rename_from(),
            expected(&new_path2).rename_to(),
            expected(tmpdir.path()).modify_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([expected(path).remove_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), subdir])
        );
    }

    #[test]
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

        rx.wait_ordered_exact([
            expected(&file1).create_any(),
            expected(&file1).modify_any().multiple(),
            expected(tmpdir.path()).modify_any(),
            expected(&file2).modify_any().multiple(),
            expected(&file1).rename_from(),
            expected(&new_path).rename_to(),
            expected(tmpdir.path()).modify_any(),
            expected(&new_path).modify_any().multiple(),
            expected(&new_path).remove_any(),
        ]);
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([
            expected(&path).rename_from(),
            expected(&new_path1).rename_to(),
            expected(tmpdir.path()).modify_any(),
            expected(&new_path1).rename_from(),
            expected(&new_path2).rename_to(),
            expected(tmpdir.path()).modify_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
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

        rx.wait_ordered_exact([expected(&path).modify_any()])
            .ensure_no_tail();
    }

    #[test]
    fn write_file_non_recursive_watch() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let path = tmpdir.path().join("entry");
        std::fs::File::create_new(&path).expect("create");

        watcher.watch_nonrecursively(&path);

        std::fs::write(&path, b"123").expect("write");

        rx.wait_ordered_exact([expected(&path).modify_any().multiple()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );
    }

    #[test]
    fn write_to_a_hardlink_pointed_to_the_file_in_the_watched_dir_doesnt_trigger_an_event() {
        let tmpdir = testdir();
        let (mut watcher, mut rx) = watcher();

        let subdir = tmpdir.path().join("subdir");
        let subdir2 = tmpdir.path().join("subdir2");
        let file = subdir.join("file");
        let hardlink = subdir2.join("hardlink");

        std::fs::create_dir(&subdir).expect("create");
        std::fs::create_dir(&subdir2).expect("create");
        std::fs::write(&file, "").expect("file");
        std::fs::hard_link(&file, &hardlink).expect("hardlink");

        watcher.watch_nonrecursively(&file);

        std::fs::write(&hardlink, "123123").expect("write to the hard link");

        let events = rx.iter().collect::<Vec<_>>();
        assert!(events.is_empty(), "unexpected events: {events:#?}");
        assert_eq!(watcher.get_watch_handles(), HashSet::from([subdir]));
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
        rx.wait_ordered_exact([
            expected(&nested1).create_any(),
            expected(&nested2).create_any(),
            expected(&nested3).create_any(),
            expected(&nested4).create_any(),
            expected(&nested5).create_any(),
            expected(&nested6).create_any(),
            expected(&nested7).create_any(),
            expected(&nested8).create_any(),
            expected(&nested9).create_any(),
        ])
        .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.parent_path_buf(), tmpdir.to_path_buf()])
        );
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

        watcher.watch_recursively(&path);
        std::fs::File::create_new(&file).expect("create");

        rx.wait_ordered_exact([expected(&deep).modify_any(), expected(&file).create_any()])
            .ensure_no_tail();
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf(), path])
        );
    }

    /// Watching 10+ sibling subdirs collapses their OS-level watch into the
    /// shared parent dir (the consolidation threshold defined on
    /// [`ConsolidatingPathTrie`] is 10).
    #[test]
    fn consolidate_many_siblings() {
        let tmpdir = testdir();
        let (mut watcher, _rx) = watcher();

        let mut subdirs = Vec::new();
        for i in 0..10 {
            let sub = tmpdir.path().join(format!("c{i}"));
            std::fs::create_dir(&sub).expect("create_dir");
            subdirs.push(sub);
        }
        let mut pm = watcher.watcher.paths_mut();
        for sub in &subdirs {
            pm.add(sub, WatchMode::recursive()).expect("paths_mut add");
        }
        pm.commit().expect("paths_mut commit");

        // Consolidation collapses the 10 sibling watches to a single recursive
        // watch on `tmpdir`. The 10 user-level `tracked_parent` entries all
        // point at `tmpdir` and so are absorbed by the consolidated primary;
        // no separate handle is opened on `tmpdir.parent_path_buf()`.
        assert_eq!(
            watcher.get_watch_handles(),
            HashSet::from([tmpdir.to_path_buf()])
        );
    }

    /// After consolidation, events for files created inside each child dir are
    /// still delivered through the consolidated parent watch.
    #[test]
    fn consolidate_delivers_child_events() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let mut subdirs = Vec::new();
        for i in 0..10 {
            let sub = tmpdir.path().join(format!("c{i}"));
            std::fs::create_dir(&sub).expect("create_dir");
            subdirs.push(sub);
        }
        let mut pm = watcher.watcher.paths_mut();
        for sub in &subdirs {
            pm.add(sub, WatchMode::recursive()).expect("paths_mut add");
        }
        pm.commit().expect("paths_mut commit");

        // Create a file inside one of the consolidated child dirs; the event
        // must still arrive even though no OS handle sits directly on `c5`.
        let file = subdirs[5].join("f");
        std::fs::File::create_new(&file).expect("create");
        rx.wait_ordered_exact([expected(&file).create_any()])
            .ensure_no_tail();
    }

    /// Mixing recursive and non-recursive watches under a shared parent
    /// consolidates them into a recursive parent watch. Events deep inside
    /// the recursive child reach the user; events deeper than 1 level inside
    /// a non-recursive child are filtered out.
    #[test]
    fn mixed_recursive_consolidates_to_recursive() {
        let tmpdir = testdir();
        let (mut watcher, rx) = watcher();

        let mut subdirs = Vec::new();
        for i in 0..10 {
            let sub = tmpdir.path().join(format!("c{i}"));
            std::fs::create_dir(&sub).expect("create_dir");
            subdirs.push(sub);
        }
        let recursive_child = subdirs[0].clone();
        let nonrecursive_child = subdirs[1].clone();
        let deep_under_rec = recursive_child.join("deep");
        std::fs::create_dir(&deep_under_rec).expect("create_dir");
        let deep_under_nonrec = nonrecursive_child.join("deep");
        std::fs::create_dir(&deep_under_nonrec).expect("create_dir");

        let mut pm = watcher.watcher.paths_mut();
        for (i, sub) in subdirs.iter().enumerate() {
            let mode = if i == 0 {
                WatchMode::recursive()
            } else {
                WatchMode::non_recursive()
            };
            pm.add(sub, mode).expect("paths_mut add");
        }
        pm.commit().expect("paths_mut commit");

        // File 1: under the recursive child
        let file_under_rec = deep_under_rec.join("f");
        std::fs::File::create_new(&file_under_rec).expect("create");

        // File 2: under the non-recursive child
        let file_under_nonrec = deep_under_nonrec.join("f");
        std::fs::File::create_new(&file_under_nonrec).expect("create");

        // We expect the deep-recursive file event but NOT the deep-nonrec one.
        rx.wait_ordered_exact([expected(&file_under_rec).create_any()])
            .ensure_no_tail();
    }
}
