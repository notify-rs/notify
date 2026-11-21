#![cfg_attr(docsrs, feature(doc_cfg))]

//! A debouncer for [notify] that is optimized for ease of use.
//!
//! * Matches `Rename` `From`/`To` events and merges rename chains
//! * Updates paths of queued events when files are renamed
//! * Emits renames before events that reuse their source paths
//! * Emits `Remove` for the source if a renamed file is deleted or replaced before delivery
//! * Optionally keeps track of the file system IDs all files and stitches rename events together (macOS FS Events, Windows)
//! * Emits only one `Remove` event when deleting a directory (inotify)
//! * Doesn't emit duplicate create events
//! * Doesn't emit `Modify` events after a `Create` event
//!
//! # Installation
//!
//! ```toml
//! [dependencies]
//! notify-debouncer-full = "0.8.0-rc.4"
//! ```
//!
//! In case you want to select specific features of notify,
//! specify notify as dependency explicitly in your dependencies.
//! Otherwise you can just use the re-export of notify from debouncer-full.
//!
//! ```toml
//! notify-debouncer-full = "0.8.0-rc.4"
//! notify = { version = "9.0.0-rc.9", features = ["serde"] }
//! ```
//!
//! # Examples
//!
//! ```rust,no_run
//! # use std::path::Path;
//! # use std::time::Duration;
//! use notify_debouncer_full::{notify::*, new_debouncer, DebounceEventResult};
//!
//! // Select recommended watcher for debouncer.
//! // Using a callback here, could also be a channel.
//! let mut debouncer = new_debouncer(Duration::from_secs(2), None, |result: DebounceEventResult| {
//!     match result {
//!         Ok(events) => events.iter().for_each(|event| println!("{event:?}")),
//!         Err(errors) => errors.iter().for_each(|error| println!("{error:?}")),
//!     }
//! }).unwrap();
//!
//! // Add a path to be watched. All files and directories at that path and
//! // below will be monitored for changes.
//! debouncer.watch(".", RecursiveMode::Recursive).unwrap();
//! ```
//!
//! # Features
//!
//! The following crate features can be configured in your Cargo dependency:
//!
//! - `macos_fsevent` (default) enables notify's FSEvents backend on macOS
//! - `freebsd_inotify` enables notify's inotify backend on FreeBSD 14.5+
//!   - inotify is automatically enabled when built natively, this feature is only needed for cross-compilation
//! - `macos_kqueue` enables notify's kqueue backend on macOS
//! - `serde` enables serialization support in notify-types
//! - `web-time` uses `web_time::Instant` for debounced events
//! - `crossbeam-channel`, `flume`, `futures`, and `tokio` enable the corresponding
//!   channel senders as event handlers
//! - `serialization-compat-6` restores notify 6 serialization behavior
//!
//! # Caveats
//!
//! Notify's [known problems](notify#known-problems), including
//! [changes to watched paths](notify#changes-to-watched-paths), also apply to debounced events.

mod cache;
mod time;

#[cfg(test)]
mod testing;

#[cfg(not(target_family = "wasm"))]
mod file_id_map;

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, VecDeque, btree_map::Entry},
    num::NonZeroU32,
    ops::Bound,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use rustc_hash::FxHashMap as HashMap;
use time::{Instant, now};

pub use cache::{FileIdCache, NoCache, RecommendedCache, WatchRoot};

#[cfg(not(target_family = "wasm"))]
pub use file_id_map::FileIdMap;

pub use file_id;
pub use notify;
pub use notify_types::debouncer_full::DebouncedEvent;

use file_id::FileId;
use notify::{
    Error, ErrorKind, Event, EventKind, EventKindMask, PathOp, RecommendedWatcher, RecursiveMode,
    UpdatePathsError, WatchedPath, Watcher, WatcherKind,
    event::{ModifyKind, RemoveKind, RenameMode},
};

/// The set of requirements for watcher debounce event handling functions.
///
/// # Example implementation
///
/// ```rust,no_run
/// # use notify::{Event, Result, EventHandler};
/// # use notify_debouncer_full::{DebounceEventHandler, DebounceEventResult};
///
/// /// Prints received events
/// struct EventPrinter;
///
/// impl DebounceEventHandler for EventPrinter {
///     fn handle_event(&mut self, result: DebounceEventResult) {
///         match result {
///             Ok(events) => events.iter().for_each(|event| println!("{event:?}")),
///             Err(errors) => errors.iter().for_each(|error| println!("{error:?}")),
///         }
///     }
/// }
/// ```
pub trait DebounceEventHandler: Send + 'static {
    /// Handles an event.
    fn handle_event(&mut self, event: DebounceEventResult);
}

impl<F> DebounceEventHandler for F
where
    F: FnMut(DebounceEventResult) + Send + 'static,
{
    fn handle_event(&mut self, event: DebounceEventResult) {
        (self)(event);
    }
}

#[cfg(feature = "crossbeam-channel")]
impl DebounceEventHandler for crossbeam_channel::Sender<DebounceEventResult> {
    fn handle_event(&mut self, event: DebounceEventResult) {
        let _ = self.send(event);
    }
}

#[cfg(feature = "flume")]
impl DebounceEventHandler for flume::Sender<DebounceEventResult> {
    fn handle_event(&mut self, event: DebounceEventResult) {
        let _ = self.send(event);
    }
}

#[cfg(feature = "futures")]
impl DebounceEventHandler for futures::channel::mpsc::UnboundedSender<DebounceEventResult> {
    fn handle_event(&mut self, event: DebounceEventResult) {
        let _ = self.unbounded_send(event);
    }
}

#[cfg(feature = "tokio")]
impl DebounceEventHandler for tokio::sync::mpsc::UnboundedSender<DebounceEventResult> {
    fn handle_event(&mut self, event: DebounceEventResult) {
        let _ = self.send(event);
    }
}

impl DebounceEventHandler for std::sync::mpsc::Sender<DebounceEventResult> {
    fn handle_event(&mut self, event: DebounceEventResult) {
        let _ = self.send(event);
    }
}

/// A result of debounced events.
/// Comes with either a vec of events or vec of errors.
pub type DebounceEventResult = Result<Vec<DebouncedEvent>, Vec<Error>>;

/// Configuration for the full debouncer.
///
/// The default backend config uses [`EventKindMask::CORE`], which excludes access events.
///
/// ```rust
/// # use std::time::Duration;
/// use notify_debouncer_full::Config;
///
/// let config = Config::default()
///     .with_timeout(Duration::from_secs(1))
///     .with_tick_rate(Duration::from_millis(100));
/// ```
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Config {
    timeout: Duration,
    tick_rate: Option<Duration>,
    notify_config: notify::Config,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(500),
            tick_rate: None,
            notify_config: notify::Config::default().with_event_kinds(EventKindMask::CORE),
        }
    }
}

impl Config {
    /// Sets the debounce timeout (default: 500 ms).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    /// Sets how often to check for events to emit.
    ///
    /// Defaults to 1/4 of the timeout and must not exceed it.
    #[must_use]
    pub fn with_tick_rate(mut self, tick_rate: Duration) -> Self {
        self.tick_rate = Some(tick_rate);
        self
    }
    /// Replace the backend [`notify::Config`], including its event mask.
    ///
    /// `notify::Config::default()` enables access events; use [`EventKindMask::CORE`] to exclude them.
    #[must_use]
    pub fn with_notify_config(mut self, notify_config: notify::Config) -> Self {
        self.notify_config = notify_config;
        self
    }
}

type DebounceData<T> = Arc<SharedDebounceData<T>>;

#[derive(Debug)]
struct SharedDebounceData<T> {
    inner: Mutex<DebounceDataInner<T>>,
    changed: Condvar,
}

const RENAME: EventKind = EventKind::Modify(ModifyKind::Name(RenameMode::Both));

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Queue {
    /// Events must be stored in the following order:
    /// 1. `remove` or `move out` event
    /// 2. `rename` event
    /// 3. Other events
    events: VecDeque<DebouncedEvent>,
}

impl Queue {
    /// A pending rename follows at most one removal.
    fn rename_index(&self) -> Option<usize> {
        self.events
            .iter()
            .take(2)
            .position(|event| event.kind == RENAME)
    }

    fn take_rename(&mut self) -> Option<DebouncedEvent> {
        self.events.remove(self.rename_index()?)
    }

    /// Drains and deduplicates expired events. The front, if any, must be expired.
    fn take_expired(
        &mut self,
        is_expired: impl Fn(&DebouncedEvent) -> bool,
    ) -> VecDeque<DebouncedEvent> {
        // Reuse the allocation when there is nothing to deduplicate.
        if self.events.len() == 1 {
            return std::mem::take(&mut self.events);
        }

        let mut kind_index: HashMap<EventKind, usize> = HashMap::default();
        let mut queue_expired = Vec::new();

        while let Some(event) = self.events.pop_front() {
            // remove previous event of the same kind
            if is_expired(&event) {
                if let Some(idx) = kind_index.insert(event.kind, queue_expired.len()) {
                    queue_expired[idx] = None;
                }

                queue_expired.push(Some(event));
            } else {
                if let Some(&idx) = kind_index.get(&event.kind) {
                    queue_expired[idx] = None;
                }
                self.events.push_front(event);
                break;
            }
        }

        queue_expired.into_iter().flatten().collect()
    }

    fn was_created(&self) -> bool {
        self.events.front().is_some_and(|event| {
            matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To))
            )
        })
    }

    fn was_removed(&self) -> bool {
        self.events.front().is_some_and(|event| {
            matches!(
                event.kind,
                EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From))
            )
        })
    }
}

/// A path vacated by a pending rename.
///
/// Nanosecond timestamps and a separate presence flag keep each entry at 24 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Vacated {
    /// Nanoseconds since debouncer creation: the rename time, or the tick that last deferred it.
    time: u64,
    /// User data from the source watch; valid when `has_user_data` is set.
    user_data: u64,
    has_user_data: bool,
    /// The rename was deferred after expiry; retrieve its source queue when it is delivered.
    waits: bool,
    /// Index in the current tick's `waiting` list, plus one; cleared before returning.
    waiting: Option<NonZeroU32>,
}

impl Vacated {
    fn new(time: u64, user_data: Option<u64>) -> Self {
        Self {
            time,
            user_data: user_data.unwrap_or_default(),
            has_user_data: user_data.is_some(),
            waits: false,
            waiting: None,
        }
    }

    fn user_data(&self) -> Option<u64> {
        self.has_user_data.then_some(self.user_data)
    }
}

/// Expired events blocked by the rename that vacated their path.
struct Waiting {
    group: usize,
    rename: Rename,
    /// Metadata to restore if this group's rename is requeued.
    vacated: Option<Vacated>,
}

/// Delivery state of the rename blocking a `Waiting` group.
enum Rename {
    Queued,
    Delivered,
    /// The rename belongs to this `Waiting` entry.
    Waits(u32),
}

/// A group released after the rename in `rename` is emitted.
struct Held {
    rename: usize,
    group: usize,
    events: VecDeque<DebouncedEvent>,
}

impl Held {
    /// Releases this group once; returns whether it contained events.
    fn release(
        &mut self,
        groups: &mut [VecDeque<DebouncedEvent>],
        min_time_heap: &mut BinaryHeap<Reverse<(Instant, usize)>>,
    ) -> bool {
        let events = std::mem::take(&mut self.events);
        let Some(event) = events.front() else {
            return false;
        };
        min_time_heap.push(Reverse((event.time, self.group)));
        groups[self.group] = events;
        true
    }
}

#[derive(Debug)]
pub(crate) struct DebounceDataInner<T> {
    queues: BTreeMap<PathBuf, Queue>,
    /// Pending rename sources, used to order events that reuse those paths.
    vacated: HashMap<Box<Path>, Vacated>,
    /// Registered watch roots, kept **sorted by path** so that `add_root`
    /// can dedupe via binary search in O(log N) and doesn't suffer from injection
    roots: VecDeque<WatchRoot>,
    cache: T,
    /// The sources of the last rename. A backend reports a rename once for each watch that
    /// covers it, so the sources of one rename share a tracker.
    rename_events: Vec<(DebouncedEvent, Option<FileId>)>,
    pending_rename_destinations: Vec<DebouncedEvent>,
    rescan_event: Option<DebouncedEvent>,
    errors: Vec<Error>,
    timeout: Duration,
    created: Instant,
    /// Whether `vacated` may contain deferred renames.
    waits: bool,
}

impl<T: FileIdCache> DebounceDataInner<T> {
    pub(crate) fn new(cache: T, timeout: Duration) -> Self {
        Self {
            queues: BTreeMap::new(),
            vacated: HashMap::default(),
            roots: VecDeque::new(),
            cache,
            rename_events: Vec::new(),
            pending_rename_destinations: Vec::new(),
            rescan_event: None,
            errors: Vec::new(),
            timeout,
            created: now(),
            waits: false,
        }
    }

    /// Nanoseconds since `created`, saturated to `u64::MAX`.
    fn nanos(&self, time: Instant) -> u64 {
        let nanos = time.saturating_duration_since(self.created).as_nanos();
        u64::try_from(nanos).unwrap_or(u64::MAX)
    }

    fn is_idle(&self) -> bool {
        self.queues.is_empty()
            && self.pending_rename_destinations.is_empty()
            && self.errors.is_empty()
            && self.rescan_event.is_none()
    }

    /// Retrieve a vec of debounced events, removing them if not continuous
    pub fn debounced_events(&mut self) -> Vec<DebouncedEvent> {
        let now = now();
        // Resolve pending renames before their source or destination queues emit older events,
        // so paths, rename chains and overwritten events are updated before delivery.
        if self
            .rename_events
            .iter()
            .map(|(event, _)| event)
            .chain(self.pending_rename_destinations.iter())
            .flat_map(|event| {
                std::iter::once(event).chain(
                    self.queues
                        .get(&event.paths[0])
                        .and_then(|queue| queue.events.front()),
                )
            })
            .any(|event| now.saturating_duration_since(event.time) >= self.timeout)
        {
            self.finish_pending_renames();
        }
        let mut groups = Vec::with_capacity(self.queues.len());

        if let Some(event) = self.rescan_event.take() {
            if now.saturating_duration_since(event.time) >= self.timeout {
                log::trace!("debounced event: {event:?}");
                groups.push(VecDeque::from([event]));
            } else {
                self.rescan_event = Some(event);
            }
        }

        let timeout = self.timeout;
        let is_expired =
            move |event: &DebouncedEvent| now.saturating_duration_since(event.time) >= timeout;
        // Expiry cutoff in the same units as `Vacated::time`.
        let timeout_nanos = u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX);
        let expired = self.nanos(now).checked_sub(timeout_nanos);

        // Visit each queue in place and remove only the ones that become empty.
        let mut waiting = Vec::new();
        self.queues.retain(|path, queue| {
            let Some(event) = queue.events.front() else {
                return false;
            };
            if !is_expired(event) {
                return true;
            }
            // Leave blocked queues intact until their rename can be emitted or rechecked.
            let vacated = self.vacated.get_mut(path.as_path());
            if let Some(vacated) = &vacated
                && expired.is_none_or(|expired| vacated.time > expired)
            {
                return true;
            }
            let events = queue.take_expired(is_expired);
            if !events.is_empty() {
                if let Some(vacated) = vacated
                    && let Ok(entry) = u32::try_from(waiting.len() + 1)
                {
                    vacated.waiting = NonZeroU32::new(entry);
                    waiting.push(Waiting {
                        group: groups.len(),
                        rename: Rename::Queued,
                        vacated: None,
                    });
                }
                groups.push(events);
            }
            !queue.events.is_empty()
        });

        if groups.is_empty() {
            return Vec::new();
        }

        let held = self.hold_waiting(&mut groups, waiting, is_expired, self.nanos(now));
        self.waits &= !self.vacated.is_empty();
        sort_events(groups, held)
    }

    /// Resolves rename dependencies and requeues groups that cannot be delivered yet.
    /// Returned groups are released by `sort_events` after their rename.
    fn hold_waiting(
        &mut self,
        groups: &mut Vec<VecDeque<DebouncedEvent>>,
        mut waiting: Vec<Waiting>,
        is_expired: impl Fn(&DebouncedEvent) -> bool + Copy,
        now: u64,
    ) -> Vec<Held> {
        if self.vacated.is_empty() {
            return Vec::new();
        }

        // Pairs of (rename group, waiting follower).
        let mut followers = Vec::new();
        let mut next = 0;
        let mut group = 0;
        while group < groups.len() {
            let own = waiting
                .get(next as usize)
                .is_some_and(|waiting| waiting.group == group)
                .then_some(next);
            next += u32::from(own.is_some());
            let rename = groups[group]
                .iter()
                .take(2)
                .find(|event| event.kind == RENAME);
            if let Some(source) = rename.map(|rename| rename.paths[0].as_path())
                && let Some(vacated) = self.vacated.remove(source)
            {
                let follower = match vacated.waiting {
                    Some(follower) => Some(follower.get() as usize - 1),
                    // Retrieve followers deferred on an earlier tick.
                    None if vacated.waits => {
                        let path = source.to_path_buf();
                        self.take_waiting(path, groups, &mut waiting, is_expired)
                    }
                    None => None,
                };
                if let Some(follower) = follower {
                    waiting[follower].rename = own.map_or(Rename::Delivered, Rename::Waits);
                    followers.push((group, follower));
                }
                if let Some(own) = own {
                    waiting[own as usize].vacated = Some(Vacated {
                        waiting: None,
                        ..vacated
                    });
                }
            }
            group += 1;
        }

        let mut delivered = vec![None; waiting.len()];
        let mut chain = Vec::new();
        for start in 0..waiting.len() {
            let mut entry = start;
            let result = loop {
                if let Some(known) = delivered[entry] {
                    break known;
                }
                // Mark as deliverable while visiting so cycles can be emitted together.
                delivered[entry] = Some(true);
                chain.push(entry);
                match waiting[entry].rename {
                    Rename::Queued => break false,
                    Rename::Delivered => break true,
                    Rename::Waits(holder) => entry = holder as usize,
                }
            };
            for entry in chain.drain(..) {
                delivered[entry] = Some(result);
            }
        }

        for (entry, waiting) in waiting.iter().enumerate() {
            if delivered[entry] != Some(true) {
                self.requeue(std::mem::take(&mut groups[waiting.group]), waiting, now);
            }
        }
        followers
            .into_iter()
            .map(|(rename, follower)| {
                let group = waiting[follower].group;
                Held {
                    rename,
                    group,
                    events: std::mem::take(&mut groups[group]),
                }
            })
            .collect()
    }

    /// Appends expired events from `path` and returns their index in `waiting`.
    fn take_waiting(
        &mut self,
        path: PathBuf,
        groups: &mut Vec<VecDeque<DebouncedEvent>>,
        waiting: &mut Vec<Waiting>,
        is_expired: impl Fn(&DebouncedEvent) -> bool,
    ) -> Option<usize> {
        let Entry::Occupied(mut queue) = self.queues.entry(path) else {
            return None;
        };
        if !queue.get().events.front().is_some_and(&is_expired) {
            return None;
        }
        let events = queue.get_mut().take_expired(is_expired);
        if queue.get().events.is_empty() {
            queue.remove();
        }
        if events.is_empty() {
            return None;
        }
        waiting.push(Waiting {
            group: groups.len(),
            rename: Rename::Queued,
            vacated: None,
        });
        groups.push(events);
        Some(waiting.len() - 1)
    }

    /// Requeues a blocked group until its rename is delivered or continues.
    /// Otherwise recheck at `now + timeout`, when all current blockers have expired.
    fn requeue(&mut self, events: VecDeque<DebouncedEvent>, waiting: &Waiting, now: u64) {
        // A rename is queued at its destination, and other events at their path.
        let rename = events.iter().take(2).find(|event| event.kind == RENAME);
        let path = match &events[0] {
            event if event.kind == RENAME => event.paths[1].clone(),
            event => event.paths[0].clone(),
        };
        self.waits = true;
        if let (Some(rename), Some(mut vacated)) = (rename, waiting.vacated) {
            (vacated.time, vacated.waits) = (now, true);
            self.vacated
                .insert(rename.paths[0].as_path().into(), vacated);
        }
        if let Rename::Queued = waiting.rename
            && let Some(vacated) = self.vacated.get_mut(path.as_path())
        {
            (vacated.time, vacated.waits, vacated.waiting) = (now, true, None);
        }
        match self.queues.entry(path) {
            Entry::Occupied(mut queue) => {
                for event in events.into_iter().rev() {
                    queue.get_mut().events.push_front(event);
                }
            }
            Entry::Vacant(queue) => {
                queue.insert(Queue { events });
            }
        }
    }

    /// Returns all currently stored errors
    pub fn errors(&mut self) -> Vec<Error> {
        std::mem::take(&mut self.errors)
    }

    /// Add an error entry to re-send later on
    pub fn add_error(&mut self, error: Error) {
        log::trace!("raw error: {error:?}");
        self.errors.push(error);
    }

    /// Add new event to debouncer cache
    pub fn add_event(&mut self, event: Event) {
        log::trace!("raw event: {event:?}");

        if self
            .pending_rename_destinations
            .first()
            .is_some_and(|pending| pending.tracker() != event.tracker())
        {
            self.finish_pending_renames();
        }

        if event.need_rescan() {
            let roots = self.roots.make_contiguous();
            self.cache.rescan(roots);
            self.rescan_event = Some(DebouncedEvent { event, time: now() });
            return;
        }

        let path = match event.paths.first() {
            Some(path) => path,
            None => {
                log::info!("skipping event with no paths: {event:?}");
                return;
            }
        };

        match &event.kind {
            EventKind::Create(_) => {
                self.add_event_path(path);

                self.push_event(event, now());
            }
            EventKind::Modify(ModifyKind::Name(rename_mode)) => {
                match rename_mode {
                    RenameMode::Any => {
                        if event.paths[0].exists() {
                            self.handle_rename_to(event);
                        } else {
                            self.handle_rename_from(event);
                        }
                    }
                    RenameMode::To => {
                        self.handle_rename_to(event);
                    }
                    RenameMode::From => {
                        self.handle_rename_from(event);
                    }
                    RenameMode::Both => {
                        // ignore and handle `To` and `From` events instead
                    }
                    RenameMode::Other => {
                        // unused
                    }
                }
            }
            EventKind::Remove(_) => {
                self.push_remove_event(event, now());
            }
            EventKind::Other => {
                // ignore meta events
            }
            _ => {
                if self.cache.cached_file_id(path).is_none() {
                    self.add_event_path(path);
                }

                self.push_event(event, now());
            }
        }
    }

    /// Adds the path of an event to the cache. A non-recursive root covers itself and its direct
    /// children, but not the entries below a child, so a child is added without them.
    fn add_event_path(&mut self, path: &Path) {
        match self.recursive_mode(path) {
            RecursiveMode::NonRecursive if root_at(&self.roots, path).is_none() => {
                self.cache.add_single_path(path)
            }
            recursive_mode => self.cache.add_path(path, recursive_mode),
        }
    }

    fn recursive_mode(&self, path: &Path) -> RecursiveMode {
        for ancestor in path.ancestors() {
            if let Ok(index) = self
                .roots
                .binary_search_by(|root| root.path.as_path().cmp(ancestor))
                && self.roots[index].recursive_mode == RecursiveMode::Recursive
            {
                return RecursiveMode::Recursive;
            }
        }

        RecursiveMode::NonRecursive
    }

    fn handle_rename_from(&mut self, event: Event) {
        let time = now();
        let path = &event.paths[0];

        // store event
        let file_id = self.cache.cached_file_id(path).map(|id| *id.as_ref());
        let tracker = event.tracker();
        if tracker.is_none()
            || self
                .rename_events
                .iter()
                .any(|(source, _)| source.tracker() != tracker)
        {
            self.rename_events.clear();
        }
        self.rename_events
            .push((DebouncedEvent::new(event.clone(), time), file_id));

        self.cache.remove_path(path);

        self.push_event(event, time);
    }

    fn handle_rename_to(&mut self, event: Event) {
        self.add_event_path(&event.paths[0]);

        let tracker = event.attrs.tracker();
        let trackers_match = |(source, _): &(DebouncedEvent, Option<FileId>)| {
            tracker.is_some() && source.tracker() == tracker
        };
        // Each watch that covers a rename reports a source and a destination. A destination is
        // paired with the source below the same root first. Two roots that reach one directory
        // through a symlink then each get a rename with their own paths.
        let destination_root = self.closest_root(&event.paths[0]);
        let source = self
            .rename_events
            .iter()
            .position(|source| {
                trackers_match(source) && self.closest_root(&source.0.paths[0]) == destination_root
            })
            .or_else(|| {
                if self.rename_events.iter().any(trackers_match) {
                    return None;
                }
                let destination_id = self.cache.cached_file_id(&event.paths[0])?;
                // A backend with trackers gives every rename its own tracker. A source with
                // another tracker belongs to an earlier rename, even when that rename moved the
                // same file.
                self.rename_events.iter().position(|(source, source_id)| {
                    (tracker.is_none() || source.tracker().is_none())
                        && source_id.as_ref() == Some(destination_id.as_ref())
                })
            });

        match source {
            Some(source) => {
                // connect rename
                let (source, _) = self.rename_events.remove(source);
                self.push_rename_event(source, event);
            }
            None if self.rename_events.iter().any(trackers_match) => {
                // An alias destination can arrive before the destination below the source's
                // root. We defer pairing across roots until the remaining destinations arrive.
                self.pending_rename_destinations
                    .push(DebouncedEvent::new(event, now()));
                return;
            }
            // move in
            None => self.push_event(event, now()),
        }

        // The destinations of the other watches follow with the same tracker.
        self.rename_events.retain(trackers_match);
    }

    fn finish_pending_renames(&mut self) {
        for destination in std::mem::take(&mut self.pending_rename_destinations) {
            let source = self
                .rename_events
                .iter()
                .position(|(source, _)| source.tracker() == destination.tracker());
            if let Some(source) = source {
                let (source, _) = self.rename_events.remove(source);
                self.push_rename_event(source, destination.event);
            } else {
                self.push_event(destination.event, destination.time);
            }
        }
    }

    /// Returns the path of the closest root at or above `path`.
    fn closest_root(&self, path: &Path) -> Option<&Path> {
        path.ancestors()
            .find_map(|ancestor| root_at(&self.roots, ancestor))
            .map(|root| root.path.as_path())
    }

    fn push_rename_event(&mut self, mut source: DebouncedEvent, mut event: Event) {
        let time = source.time;
        let path = source.event.paths.swap_remove(0);
        self.cache.remove_path(&path);

        let (vacated, mut source_queue) = self
            .queues
            .remove_entry(&path)
            .unwrap_or_else(|| (path.clone(), Queue::default()));

        // remove rename `from` event
        source_queue.events.pop_back();

        // Attribute replacement removals to the destination watch.
        let user_data = event.user_data();

        // remove existing rename event
        let earlier = source_queue.take_rename();
        let original_time = earlier.as_ref().map_or(time, |rename| rename.time);

        // split off remove or move out event and add it back to the events map
        if source_queue.was_removed() {
            let event = source_queue.events.pop_front().unwrap();

            self.queues.insert(
                event.paths[0].clone(),
                Queue {
                    events: [event].into(),
                },
            );
        }

        // update paths
        for e in &mut source_queue.events {
            e.paths = vec![event.paths[0].clone()];
        }

        // insert rename event at the front, unless the file was just created
        if !source_queue.was_created() {
            let original_path = match earlier {
                // A rename chain keeps its original source.
                Some(mut rename) => {
                    let original_path = rename.event.paths.swap_remove(0);
                    // The new destination may change dependencies; invalidate the deferred state.
                    if self.waits {
                        let time = self.nanos(original_time);
                        if let Some(vacated) = self.vacated.get_mut(original_path.as_path()) {
                            (vacated.time, vacated.waits) = (time, false);
                        }
                    }
                    original_path
                }
                None => {
                    let rename = Vacated::new(self.nanos(time), source.user_data());
                    self.vacated.insert(vacated.into_boxed_path(), rename);
                    path
                }
            };
            source_queue.events.push_front(DebouncedEvent {
                event: Event {
                    kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                    paths: vec![original_path, event.paths[0].clone()],
                    attrs: event.attrs,
                },
                time: original_time,
            });
        } else if let Some(rename) = earlier {
            // The file is reported as created at its new path, without its earlier rename.
            self.discard_rename(rename, &path);
        }

        let overwritten = match self.queues.entry(event.paths.swap_remove(0)) {
            Entry::Occupied(mut target) => {
                if !target.get().was_created() {
                    let mut removal = removal(target.key().clone(), user_data);
                    if !target.get().was_removed() {
                        removal = removal.set_info("override");
                    }
                    source_queue
                        .events
                        .push_front(DebouncedEvent::new(removal, original_time));
                }
                let rename = target.insert(source_queue).take_rename();
                rename.map(|rename| (rename, target.key().clone()))
            }
            Entry::Vacant(target) => {
                target.insert(source_queue);
                None
            }
        };
        if let Some((rename, destination)) = overwritten {
            self.discard_rename(rename, &destination);
        }
    }

    /// Replaces a discarded rename with removal of its source.
    /// `removed` is the deleted or replaced destination, or an ancestor directory.
    fn discard_rename(&mut self, mut rename: DebouncedEvent, removed: &Path) {
        let source = rename.event.paths.swap_remove(0);
        let vacated = self.vacated.remove(source.as_path());
        // The removal of a directory covers the paths below it.
        if source.starts_with(removed) {
            return;
        }
        let user_data = vacated.and_then(|vacated| vacated.user_data());
        let removal =
            |path: &PathBuf| DebouncedEvent::new(removal(path.clone(), user_data), rename.time);
        match self.queues.entry(source) {
            Entry::Occupied(queue) if queue.get().was_removed() => {}
            // Keep later events, such as recreation of the source.
            Entry::Occupied(mut queue) => {
                let removal = removal(queue.key());
                queue.get_mut().events.push_front(removal);
            }
            Entry::Vacant(queue) => {
                let events = [removal(queue.key())].into();
                queue.insert(Queue { events });
            }
        }
    }

    fn push_remove_event(&mut self, event: Event, time: Instant) {
        let path = &event.paths[0];

        // Replace this path's queue and discard any pending events below it.
        let mut discarded = Vec::new();
        let has_renames = !self.vacated.is_empty();
        remove_subtree(&mut self.queues, path, |mut queue| {
            if has_renames {
                discarded.extend(queue.take_rename());
            }
        });
        for rename in discarded {
            self.discard_rename(rename, path);
        }
        self.cache.remove_path(path);
        self.queues.insert(
            path.to_path_buf(),
            Queue {
                events: [DebouncedEvent::new(event, time)].into(),
            },
        );
    }

    fn push_event(&mut self, event: Event, time: Instant) {
        let path = &event.paths[0];

        match self.queues.entry(path.to_path_buf()) {
            Entry::Occupied(mut entry) => {
                let queue = entry.get_mut();
                // Skip duplicate create events and modifications right after creation.
                // This code relies on backends never emitting a `Modify` event with kind other than `Name` for a rename event.
                if match event.kind {
                    EventKind::Modify(
                        ModifyKind::Any
                        | ModifyKind::Data(_)
                        | ModifyKind::Metadata(_)
                        | ModifyKind::Other,
                    )
                    | EventKind::Create(_) => !queue.was_created(),
                    _ => true,
                } {
                    queue.events.push_back(DebouncedEvent::new(event, time));
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(Queue {
                    events: [DebouncedEvent::new(event, time)].into(),
                });
            }
        }
    }
}

/// Debouncer guard, stops the debouncer on drop.
#[derive(Debug)]
pub struct Debouncer<T: Watcher, C: FileIdCache> {
    watcher: T,
    debouncer_thread: Option<std::thread::JoinHandle<()>>,
    data: DebounceData<C>,
    stop: Arc<AtomicBool>,
}

impl<T: Watcher, C: FileIdCache> Debouncer<T, C> {
    /// Stop the debouncer, waking the event thread and waiting for it to finish.
    /// This also waits for any in-progress event handler to return.
    pub fn stop(mut self) {
        self.set_stop();
        if let Some(t) = self.debouncer_thread.take() {
            let _ = t.join();
        }
    }

    /// Stop the debouncer, does not wait for the event thread to finish.
    pub fn stop_nonblocking(self) {
        self.set_stop();
    }

    fn set_stop(&self) {
        let _lock = self.data.inner.lock().unwrap();
        self.stop.store(true, Ordering::Relaxed);
        self.data.changed.notify_all();
    }

    fn add_root(&mut self, path: impl Into<PathBuf>, recursive_mode: RecursiveMode) {
        let path = path.into();

        let mut data = self.data.inner.lock().unwrap();

        match data
            .roots
            .binary_search_by(|root| root.path.as_path().cmp(path.as_path()))
        {
            Ok(_) => return, // already registered
            Err(pos) => {
                // `VecDeque::insert` is O(min(pos, len - pos))
                data.roots.insert(
                    pos,
                    WatchRoot {
                        path: path.clone(),
                        recursive_mode,
                    },
                );
            }
        }

        data.cache.add_path(&path, recursive_mode);
    }

    fn remove_root(&mut self, path: impl AsRef<Path>) {
        let mut data = self.data.inner.lock().unwrap();
        let path = path.as_ref();

        // Unwatching a path only removes that watch, so roots nested under it are still
        // watched and must keep their bookkeeping. Their cached IDs go away with the
        // subtree drop below, so re-fingerprint each survivor afterwards.
        let surviving_nested_roots: Vec<_> = data
            .roots
            .iter()
            .filter(|root| root.path.starts_with(path) && root.path.as_path() != path)
            .cloned()
            .collect();

        data.roots.retain(|root| root.path.as_path() != path);

        data.cache.remove_path(path);
        for root in surviving_nested_roots {
            data.cache.add_path(&root.path, root.recursive_mode);
        }
    }

    pub fn watch(
        &mut self,
        path: impl AsRef<Path>,
        recursive_mode: RecursiveMode,
    ) -> notify::Result<()> {
        self.watcher.watch(path.as_ref(), recursive_mode)?;
        self.add_root(path.as_ref(), recursive_mode);
        Ok(())
    }

    pub fn unwatch(&mut self, path: impl AsRef<Path>) -> notify::Result<()> {
        self.watcher.unwatch(path.as_ref())?;
        self.remove_root(path);
        Ok(())
    }

    pub fn watched_paths(&self) -> notify::Result<Vec<WatchedPath>> {
        self.watcher.watched_paths()
    }

    /// Add/remove paths to watch in batch.
    ///
    /// For some [`Watcher`] implementations this method provides better performance than multiple
    /// calls to [`Watcher::watch`] and [`Watcher::unwatch`] if you want to add/remove many paths at once.
    ///
    /// # Errors
    ///
    /// Returns [`UpdatePathsError`] if any operation fails. Operations are applied sequentially.
    /// When an error occurs, processing stops: operations before `origin` have been applied,
    /// `origin` is the operation that failed (if known), and `remaining` are the operations that
    /// were not attempted. `remaining` does not include `origin`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use notify::{Watcher, RecursiveMode, PathOp};
    /// # use notify_debouncer_full::{Config, RecommendedCache, new_debouncer_opt};
    /// # use std::path::{Path, PathBuf};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let mut debouncer = new_debouncer_opt::<_, notify::NullWatcher, _>(
    /// #     Config::default(),
    /// #     |e| {},
    /// #     RecommendedCache::new(),
    /// # )?;
    /// debouncer.update_paths([
    ///     PathOp::watch_recursive("path/to/file"),
    ///     PathOp::unwatch("path/to/file2"),
    /// ])?;
    /// # Ok(())
    /// # }
    /// ```
    #[expect(
        clippy::result_large_err,
        reason = "Preserve the underlying watcher's public error, including the failed operation's config"
    )]
    pub fn update_paths<Op: Into<PathOp>>(
        &mut self,
        ops: impl IntoIterator<Item = Op>,
    ) -> std::result::Result<(), UpdatePathsError> {
        // One entry per operation: the root update it implies, or `None` for an operation
        // this version does not know and therefore only forwards to the watcher.
        let mut paths = Vec::new();
        let ops: Vec<_> = ops
            .into_iter()
            .map(Into::into)
            .inspect(|op| {
                paths.push(match op {
                    PathOp::Watch(path, config) => {
                        Some((path.clone(), Some(config.recursive_mode())))
                    }
                    PathOp::Unwatch(path) => Some((path.clone(), None)),
                    _ => None,
                });
            })
            .collect();

        let res = self.watcher.update_paths(ops);
        let updated_len = match res.as_ref() {
            Ok(()) => paths.len(),
            Err(e) => {
                let failed = usize::from(e.origin.is_some());
                paths.len().saturating_sub(e.remaining.len() + failed)
            }
        };
        for (path, watch_mode) in paths[..updated_len].iter().flatten() {
            match watch_mode {
                Some(recursive_mode) => self.add_root(path, *recursive_mode),
                None => self.remove_root(path),
            }
        }
        res
    }

    pub fn configure(&mut self, option: notify::Config) -> notify::Result<bool> {
        self.watcher.configure(option)
    }

    #[must_use]
    pub fn kind() -> WatcherKind
    where
        Self: Sized,
    {
        T::kind()
    }
}

impl<T: Watcher, C: FileIdCache> Drop for Debouncer<T, C> {
    fn drop(&mut self) {
        self.set_stop();
    }
}

/// Creates a new debounced watcher with custom configuration.
///
/// # Errors
///
/// Returns an error if the tick rate of `config` exceeds its timeout, or if the watcher
/// cannot be created.
pub fn new_debouncer_opt<F: DebounceEventHandler, T: Watcher, C: FileIdCache + Send + 'static>(
    config: Config,
    mut event_handler: F,
    file_id_cache: C,
) -> Result<Debouncer<T, C>, Error> {
    let Config {
        timeout,
        tick_rate,
        notify_config,
    } = config;
    let data = Arc::new(SharedDebounceData {
        inner: Mutex::new(DebounceDataInner::new(file_id_cache, timeout)),
        changed: Condvar::new(),
    });
    let stop = Arc::new(AtomicBool::new(false));

    let tick_div = 4;
    let tick = match tick_rate {
        Some(v) => {
            if v > timeout {
                return Err(Error::new(ErrorKind::Generic(format!(
                    "Invalid tick_rate, tick rate {v:?} > {timeout:?} timeout!"
                ))));
            }
            v
        }
        None => timeout.checked_div(tick_div).ok_or_else(|| {
            Error::new(ErrorKind::Generic(format!(
                "Failed to calculate tick as {timeout:?}/{tick_div}!"
            )))
        })?,
    };

    let data_c = data.clone();
    let watcher = T::new(
        move |e: Result<Event, Error>| {
            let mut lock = data_c.inner.lock().unwrap();
            let was_idle = lock.is_idle();
            match e {
                Ok(e) => lock.add_event(e),
                // can't have multiple TX, so we need to pipe that through our debouncer
                Err(e) => lock.add_error(e),
            }
            // The worker only waits while idle. Once work is pending, it already
            // wakes on its debounce tick, so more events need no notification.
            if was_idle && !lock.is_idle() {
                data_c.changed.notify_one();
            }
        },
        notify_config,
    )?;

    let data_c = data.clone();
    let stop_c = stop.clone();
    let thread = std::thread::Builder::new()
        .name("notify-rs debouncer loop".to_string())
        .spawn(move || {
            loop {
                let mut lock = data_c.inner.lock().unwrap();
                while lock.is_idle() && !stop_c.load(Ordering::Acquire) {
                    lock = data_c.changed.wait(lock).unwrap();
                }
                if stop_c.load(Ordering::Acquire) {
                    break;
                }
                drop(lock);
                std::thread::sleep(tick);
                if stop_c.load(Ordering::Acquire) {
                    break;
                }
                lock = data_c.inner.lock().unwrap();
                let send_data = lock.debounced_events();
                let errors = lock.errors();
                drop(lock);
                if !send_data.is_empty() {
                    event_handler.handle_event(Ok(send_data));
                }
                if !errors.is_empty() {
                    event_handler.handle_event(Err(errors));
                }
            }
        })?;

    let guard = Debouncer {
        watcher,
        debouncer_thread: Some(thread),
        data,
        stop,
    };

    Ok(guard)
}

/// Creates a debouncer with the recommended watcher and built-in file ID cache.
///
/// `timeout` sets the debounce delay; `tick_rate` defaults to 1/4 of it.
///
/// Uses [`EventKindMask::CORE`], excluding access events such as `Access(Close(Write))`.
/// To customize the event mask, use [`new_debouncer_opt`].
pub fn new_debouncer<F: DebounceEventHandler>(
    timeout: Duration,
    tick_rate: Option<Duration>,
    event_handler: F,
) -> Result<Debouncer<RecommendedWatcher, RecommendedCache>, Error> {
    let config = Config {
        timeout,
        tick_rate,
        ..Config::default()
    };
    new_debouncer_opt::<F, RecommendedWatcher, RecommendedCache>(
        config,
        event_handler,
        RecommendedCache::new(),
    )
}

fn root_at<'a>(roots: &'a VecDeque<WatchRoot>, path: &Path) -> Option<&'a WatchRoot> {
    let index = roots
        .binary_search_by(|root| root.path.as_path().cmp(path))
        .ok()?;
    Some(&roots[index])
}

/// Removes a subtree without scanning unrelated keys, passing each value to `removed`.
fn remove_subtree<V>(map: &mut BTreeMap<PathBuf, V>, path: &Path, mut removed: impl FnMut(V)) {
    // Path orders by component, so a path and its descendants form a contiguous range.
    let count = map
        .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
        .take_while(|(key, _)| key.starts_with(path))
        .count();
    if count == 0 {
        return;
    }

    // A false predicate would keep scanning unrelated keys. Stop after the counted subtree
    // instead, removing entries in place without cloning their keys or looking them up again.
    map.extract_if(path.to_path_buf().., |_, _| true)
        .take(count)
        .for_each(|(_, value)| removed(value));
}

fn removal(path: PathBuf, user_data: Option<u64>) -> Event {
    let event = Event::new(EventKind::Remove(RemoveKind::Any)).add_path(path);
    match user_data {
        Some(user_data) => event.set_user_data(user_data),
        None => event,
    }
}

/// Merges groups by time, preserving each queue's order and releasing `held` followers.
/// `held` must be sorted by rename group. Timestamp ties use group order.
fn sort_events(
    mut groups: Vec<VecDeque<DebouncedEvent>>,
    mut held: Vec<Held>,
) -> Vec<DebouncedEvent> {
    let len = groups.iter().map(VecDeque::len).sum::<usize>()
        + held.iter().map(|held| held.events.len()).sum::<usize>();
    let mut sorted = Vec::with_capacity(len);

    let mut min_times = Vec::with_capacity(groups.len());
    min_times.extend(
        groups
            .iter()
            .enumerate()
            .filter_map(|(index, events)| Some(Reverse((events.front()?.time, index)))),
    );
    let mut min_time_heap = BinaryHeap::from(min_times);

    loop {
        while let Some(Reverse((min_time, index))) = min_time_heap.pop() {
            let mut push_next = false;

            while groups[index]
                .front()
                .is_some_and(|event| event.time <= min_time)
            {
                let event = groups[index].pop_front().unwrap();
                if event.kind == RENAME
                    && let Ok(follower) = held.binary_search_by_key(&index, |held| held.rename)
                {
                    held[follower].release(&mut groups, &mut min_time_heap);
                }
                sorted.push(event);
                push_next = true;
            }

            if push_next && let Some(event) = groups[index].front() {
                min_time_heap.push(Reverse((event.time, index)));
            }
        }

        // Release remaining groups together to break rename cycles.
        let mut released = false;
        for held in &mut held {
            released |= held.release(&mut groups, &mut min_time_heap);
        }
        if !released {
            break;
        }
    }

    sorted
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::Duration,
    };

    use super::*;

    use notify::event::{CreateKind, DataChange};
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use tempfile::tempdir;
    use testing::TestCase;
    use time::MockTime;

    #[rstest]
    #[case::root_and_descendants(
        "a/b",
        &["a/b/second", "a/bc", "a", "a/b/first/deep", "a/b", "a/c"],
        &["a", "a/bc", "a/c"],
    )]
    #[case::descendants_without_root(
        "a/b",
        &["a/b/second", "a/bc", "a", "a/b/first/deep", "a/c"],
        &["a", "a/bc", "a/c"],
    )]
    #[case::missing_subtree("a/missing", &["a", "a/b", "a/bc"], &["a", "a/b", "a/bc"])]
    #[case::missing_before_other_keys("a/b", &["a/bc", "a/c"], &["a/bc", "a/c"])]
    #[case::subtree_at_end("a/b", &["a", "a/b/child", "a/b"], &["a"])]
    #[case::empty_map("a/b", &[], &[])]
    fn remove_subtree_keeps_unrelated_paths(
        #[case] root: &str,
        #[case] paths: &[&str],
        #[case] remaining: &[&str],
    ) {
        let mut map = paths.iter().map(|path| (PathBuf::from(path), ())).collect();
        remove_subtree(&mut map, Path::new(root), drop);
        let expected = remaining
            .iter()
            .map(|path| (PathBuf::from(path), ()))
            .collect();
        assert_eq!(map, expected);
    }

    #[test]
    fn removing_a_large_subtree_keeps_the_following_subtree() {
        let mut map = BTreeMap::new();
        for index in 0..64 {
            map.insert(PathBuf::from(format!("a/b/{index}")), index);
            map.insert(PathBuf::from(format!("a/bc/{index}")), index);
        }

        remove_subtree(&mut map, Path::new("a/b"), drop);

        assert_eq!(map.len(), 64);
        for index in 0..64 {
            assert_eq!(map[&PathBuf::from(format!("a/bc/{index}"))], index);
        }
    }

    #[test]
    fn removing_a_parent_without_a_queue_discards_child_events() {
        let mut state = DebounceDataInner::new(NoCache, Duration::ZERO);
        for path in ["a/b/child", "a/b/deep/leaf", "a/bc"] {
            state.add_event(
                Event::new(EventKind::Modify(ModifyKind::Any)).add_path(PathBuf::from(path)),
            );
        }
        let removal = Event::new(EventKind::Remove(RemoveKind::Any))
            .add_path(PathBuf::from("a/b"))
            .set_user_data(7);
        state.add_event(removal.clone());

        let events: Vec<_> = state
            .debounced_events()
            .into_iter()
            .map(|event| event.event)
            .collect();
        assert_eq!(events.len(), 2);
        assert!(events.contains(&removal));
        assert!(events.contains(
            &Event::new(EventKind::Modify(ModifyKind::Any)).add_path(PathBuf::from("a/bc")),
        ));
        assert!(state.queues.is_empty());
    }

    #[derive(Debug)]
    struct FailingWatcher {
        fail_path: PathBuf,
    }

    impl Watcher for FailingWatcher {
        fn new<F: notify::EventHandler>(
            _event_handler: F,
            _config: notify::Config,
        ) -> notify::Result<Self> {
            Ok(Self {
                fail_path: PathBuf::from("bad"),
            })
        }

        fn watch(&mut self, path: &Path, _recursive_mode: RecursiveMode) -> notify::Result<()> {
            if path == self.fail_path {
                Err(Error::path_not_found())
            } else {
                Ok(())
            }
        }

        fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
            if path == self.fail_path {
                Err(Error::path_not_found())
            } else {
                Ok(())
            }
        }

        fn kind() -> WatcherKind {
            WatcherKind::NullWatcher
        }
    }

    #[derive(Debug)]
    struct UnconstructibleWatcher;

    impl Watcher for UnconstructibleWatcher {
        fn new<F: notify::EventHandler>(
            _event_handler: F,
            _config: notify::Config,
        ) -> notify::Result<Self> {
            Err(Error::generic("watcher construction failed"))
        }

        fn watch(&mut self, _path: &Path, _recursive_mode: RecursiveMode) -> notify::Result<()> {
            unreachable!()
        }

        fn unwatch(&mut self, _path: &Path) -> notify::Result<()> {
            unreachable!()
        }

        fn kind() -> WatcherKind {
            WatcherKind::NullWatcher
        }
    }

    #[derive(Debug, Default)]
    struct TrackingWatcher {
        watched: Vec<WatchedPath>,
    }

    fn test_config() -> Config {
        Config::default()
            .with_timeout(Duration::from_millis(20))
            .with_tick_rate(Duration::from_millis(5))
    }

    fn tracking_debouncer<C: FileIdCache + Send + 'static>(
        cache: C,
    ) -> Result<Debouncer<TrackingWatcher, C>, Error> {
        new_debouncer_opt(test_config(), |_| {}, cache)
    }

    impl Watcher for TrackingWatcher {
        fn new<F: notify::EventHandler>(
            _event_handler: F,
            _config: notify::Config,
        ) -> notify::Result<Self> {
            Ok(Self::default())
        }

        fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> notify::Result<()> {
            self.watched
                .push(WatchedPath::new(path.to_path_buf(), recursive_mode));
            Ok(())
        }

        fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
            let original_len = self.watched.len();
            self.watched.retain(|watched| watched.path != path);

            if self.watched.len() == original_len {
                Err(Error::watch_not_found())
            } else {
                Ok(())
            }
        }

        fn kind() -> WatcherKind {
            WatcherKind::NullWatcher
        }

        fn watched_paths(&self) -> notify::Result<Vec<WatchedPath>> {
            Ok(self.watched.clone())
        }
    }

    #[rstest]
    fn state(
        #[values(
            "add_create_and_remove_event",
            "add_create_event",
            "add_create_event_after_remove_event",
            "add_create_dir_event_twice",
            "add_event_with_no_paths_is_ok",
            "add_modify_any_event_after_create_event",
            "add_modify_content_event_after_create_event",
            "add_rename_from_event",
            "add_rename_from_event_after_create_event",
            "add_rename_from_event_after_modify_event",
            "add_rename_from_event_after_create_and_modify_event",
            "add_rename_from_event_after_rename_from_event",
            "add_rename_to_event",
            "add_rename_to_dir_event",
            "add_rename_from_and_to_event",
            "add_rename_from_and_to_event_after_create",
            "add_rename_from_and_to_event_after_rename",
            "add_rename_from_and_to_event_after_modify_content",
            "add_rename_from_and_to_event_override_created",
            "add_rename_from_and_to_event_override_modified",
            "add_rename_from_and_to_event_override_removed",
            "add_rename_from_and_to_event_with_file_ids",
            "add_rename_from_and_to_event_with_different_file_ids",
            "add_rename_from_and_to_event_with_different_tracker",
            "add_rename_from_and_to_event_with_file_ids_and_different_trackers",
            "add_rename_both_event",
            "add_remove_event",
            "add_remove_event_after_create_event",
            "add_remove_event_after_modify_event",
            "add_remove_event_after_create_and_modify_event",
            "add_remove_parent_event_after_remove_child_event",
            "add_remove_event_after_rename_event",
            "add_remove_event_after_rename_and_create_event",
            "add_remove_parent_event_after_rename_event",
            "add_errors",
            "debounce_modify_events",
            "emit_continuous_modify_content_events",
            "emit_events_in_chronological_order",
            "emit_events_with_a_prepended_rename_event",
            "emit_create_event_after_rename_from_same_path",
            "emit_create_event_after_rename_in_waiting_queue",
            "emit_close_events_only_once",
            "emit_modify_event_after_close_event",
            "emit_needs_rescan_event",
            "read_file_id_without_create_event",
            "sort_events_chronologically",
            "sort_events_with_reordering"
        )]
        file_name: &str,
    ) {
        let file_content =
            fs::read_to_string(Path::new(&format!("./test_cases/{file_name}.hjson"))).unwrap();
        let mut test_case = deser_hjson::from_str::<TestCase>(&file_content).unwrap();

        let time = now();
        MockTime::set_time(time);

        let mut state = test_case.state.into_debounce_data_inner(time);
        state.roots = VecDeque::from([WatchRoot {
            path: PathBuf::from("/"),
            recursive_mode: RecursiveMode::Recursive,
        }]);

        let mut prev_event_time = Duration::default();

        for event in test_case.events {
            let event_time = Duration::from_millis(event.time);
            let event = event.into_debounced_event(time, None);
            MockTime::advance(event_time - prev_event_time);
            prev_event_time = event_time;
            state.add_event(event.event);
        }

        for error in test_case.errors {
            let error = error.into_notify_error();
            state.add_error(error);
        }

        let expected_errors = std::mem::take(&mut test_case.expected.errors);
        let expected_events = std::mem::take(&mut test_case.expected.events);
        let expected_state = test_case.expected.into_debounce_data_inner(time);
        assert_eq!(
            state.queues, expected_state.queues,
            "queues not as expected"
        );
        testing::assert_vacated_paths(&state, &"after the events");
        assert_eq!(
            state.rename_events, expected_state.rename_events,
            "rename event not as expected"
        );
        assert_eq!(
            state.rescan_event, expected_state.rescan_event,
            "rescan event not as expected"
        );
        assert_eq!(
            state.cache.paths, expected_state.cache.paths,
            "cache not as expected"
        );

        assert_eq!(
            state
                .errors
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>(),
            expected_errors
                .iter()
                .map(|e| format!("{:?}", e.clone().into_notify_error()))
                .collect::<Vec<_>>(),
            "errors not as expected"
        );

        let backup_time = now();
        let backup_queues = state.queues.clone();
        let backup_vacated = state.vacated.clone();

        for (delay, events) in expected_events {
            MockTime::set_time(backup_time);
            state.queues = backup_queues.clone();
            state.vacated = backup_vacated.clone();

            match delay.as_str() {
                "none" => {}
                "short" => MockTime::advance(Duration::from_millis(10)),
                "long" => MockTime::advance(Duration::from_millis(100)),
                _ => {
                    if let Ok(ts) = delay.parse::<u64>() {
                        MockTime::set_time(time + Duration::from_millis(ts));
                    }
                }
            }

            let events = events
                .into_iter()
                .map(|event| event.into_debounced_event(time, None))
                .collect::<Vec<_>>();

            assert_eq!(
                state.debounced_events(),
                events,
                "debounced events after a `{delay}` delay"
            );
            testing::assert_vacated_paths(&state, &delay);
        }
    }

    #[test]
    fn a_rename_reported_for_two_roots_pairs_the_paths_of_each_root() {
        let mut state = DebounceDataInner::new(NoCache, Duration::from_millis(50));
        state.roots = VecDeque::from([
            WatchRoot {
                path: PathBuf::from("/alias"),
                recursive_mode: RecursiveMode::Recursive,
            },
            WatchRoot {
                path: PathBuf::from("/real"),
                recursive_mode: RecursiveMode::Recursive,
            },
        ]);
        // `/alias` is a symlink to `/real`, and the backend reports the rename for each root.
        let rename = |mode, path: &str| {
            Event::new(EventKind::Modify(ModifyKind::Name(mode)))
                .add_path(PathBuf::from(path))
                .set_tracker(1)
        };
        state.add_event(rename(RenameMode::From, "/real/from"));
        state.add_event(rename(RenameMode::From, "/alias/from"));
        state.add_event(rename(RenameMode::To, "/real/to"));
        state.add_event(rename(RenameMode::To, "/alias/to"));

        for root in ["/alias", "/real"] {
            let to = Path::new(root).join("to");
            let events = &state.queues[&to].events;
            assert!(
                events.iter().any(|event| event.kind
                    == EventKind::Modify(ModifyKind::Name(RenameMode::Both))
                    && event.paths == [Path::new(root).join("from"), to.clone()]),
                "{events:?}"
            );
        }
    }

    #[test]
    fn a_rename_between_two_roots_is_paired() {
        MockTime::set_time(Instant::now());
        let mut state = DebounceDataInner::new(NoCache, Duration::from_millis(50));
        state.roots = VecDeque::from([
            WatchRoot {
                path: PathBuf::from("/a"),
                recursive_mode: RecursiveMode::Recursive,
            },
            WatchRoot {
                path: PathBuf::from("/b"),
                recursive_mode: RecursiveMode::Recursive,
            },
        ]);
        let rename = |mode, path: &str| {
            Event::new(EventKind::Modify(ModifyKind::Name(mode)))
                .add_path(PathBuf::from(path))
                .set_tracker(1)
        };
        state.add_event(rename(RenameMode::From, "/a/from"));
        state.add_event(rename(RenameMode::To, "/b/to"));
        MockTime::advance(Duration::from_millis(50));
        let events = state.debounced_events();
        assert!(
            events.iter().any(|event| event.kind
                == EventKind::Modify(ModifyKind::Name(RenameMode::Both))
                && event.paths == [PathBuf::from("/a/from"), PathBuf::from("/b/to")]),
            "{events:?}"
        );
    }

    #[rstest]
    #[case::created_source("/a/from", EventKind::Create(CreateKind::File))]
    #[case::modified_source("/a/from", EventKind::Modify(ModifyKind::Data(DataChange::Content)))]
    #[case::created_destination("/b/to", EventKind::Create(CreateKind::File))]
    #[case::modified_destination("/b/to", EventKind::Modify(ModifyKind::Data(DataChange::Content)))]
    #[case::removed_destination("/b/to", EventKind::Remove(RemoveKind::File))]
    fn a_pending_rename_preserves_earlier_events(
        #[case] initial_path: &str,
        #[case] initial_kind: EventKind,
    ) {
        MockTime::set_time(Instant::now());
        // Pair immediately under one root, and defer the same rename between two roots.
        let mut states = [vec!["/"], vec!["/a", "/b"]].map(|roots| {
            let mut state = DebounceDataInner::new(NoCache, Duration::from_millis(50));
            state.roots = roots
                .into_iter()
                .map(|path| WatchRoot {
                    path: PathBuf::from(path),
                    recursive_mode: RecursiveMode::Recursive,
                })
                .collect();
            state.add_event(Event::new(initial_kind).add_path(PathBuf::from(initial_path)));
            state
        });

        MockTime::advance(Duration::from_millis(40));
        for state in &mut states {
            for (mode, path) in [(RenameMode::From, "/a/from"), (RenameMode::To, "/b/to")] {
                state.add_event(
                    Event::new(EventKind::Modify(ModifyKind::Name(mode)))
                        .add_path(PathBuf::from(path))
                        .set_tracker(1),
                );
            }
        }

        // Check before and at both deadlines, then ensure nothing remains to be delivered.
        let [immediate, pending] = &mut states;
        for delay in [9, 1, 39, 1, 50] {
            MockTime::advance(Duration::from_millis(delay));
            assert_eq!(pending.debounced_events(), immediate.debounced_events());
        }
    }

    #[test]
    fn a_pending_rename_preserves_the_original_path_and_time() {
        let time = Instant::now();
        MockTime::set_time(time);
        let mut state = DebounceDataInner::new(NoCache, Duration::from_millis(50));
        state.roots = ["/a", "/b"]
            .map(|path| WatchRoot {
                path: PathBuf::from(path),
                recursive_mode: RecursiveMode::Recursive,
            })
            .into();
        let rename = |mode, path: &str, tracker| {
            Event::new(EventKind::Modify(ModifyKind::Name(mode)))
                .add_path(PathBuf::from(path))
                .set_tracker(tracker)
        };
        state.add_event(rename(RenameMode::From, "/a/original", 1));
        state.add_event(rename(RenameMode::To, "/a/intermediate", 1));
        MockTime::advance(Duration::from_millis(40));
        state.add_event(rename(RenameMode::From, "/a/intermediate", 2));
        state.add_event(rename(RenameMode::To, "/b/final", 2));

        MockTime::advance(Duration::from_millis(9));
        assert!(state.debounced_events().is_empty());
        MockTime::advance(Duration::from_millis(1));
        assert_eq!(
            state.debounced_events(),
            [DebouncedEvent::new(
                rename(RenameMode::Both, "/a/original", 2).add_path(PathBuf::from("/b/final")),
                time,
            )]
        );
        MockTime::advance(Duration::from_millis(40));
        assert!(state.debounced_events().is_empty());
    }

    #[test]
    fn a_new_tracker_finishes_a_pending_rename_between_roots() {
        let mut state = DebounceDataInner::new(NoCache, Duration::from_millis(50));
        state.roots = VecDeque::from([
            WatchRoot {
                path: PathBuf::from("/a"),
                recursive_mode: RecursiveMode::Recursive,
            },
            WatchRoot {
                path: PathBuf::from("/b"),
                recursive_mode: RecursiveMode::Recursive,
            },
        ]);
        let rename = |mode, path: &str, tracker| {
            Event::new(EventKind::Modify(ModifyKind::Name(mode)))
                .add_path(PathBuf::from(path))
                .set_tracker(tracker)
        };
        state.add_event(rename(RenameMode::From, "/a/from", 1));
        state.add_event(rename(RenameMode::To, "/b/to", 1));
        state.add_event(rename(RenameMode::From, "/b/to", 2));
        state.add_event(rename(RenameMode::To, "/b/final", 2));

        assert_eq!(
            state.queues[Path::new("/b/final")].events[0].paths,
            [PathBuf::from("/a/from"), PathBuf::from("/b/final")]
        );
    }

    #[test]
    fn an_alias_destination_does_not_take_a_later_same_root_source() {
        for alias_first in [false, true] {
            let time = Instant::now();
            MockTime::set_time(time);
            let mut state = DebounceDataInner::new(NoCache, Duration::from_millis(50));
            state.roots = VecDeque::from([
                WatchRoot {
                    path: PathBuf::from("/alias"),
                    recursive_mode: RecursiveMode::Recursive,
                },
                WatchRoot {
                    path: PathBuf::from("/real"),
                    recursive_mode: RecursiveMode::Recursive,
                },
            ]);
            let rename = |mode, path: &str| {
                Event::new(EventKind::Modify(ModifyKind::Name(mode)))
                    .add_path(PathBuf::from(path))
                    .set_tracker(1)
            };
            let unrelated = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                .add_path(PathBuf::from("/real/unrelated"));
            state.add_event(unrelated.clone());
            MockTime::advance(Duration::from_millis(40));
            state.add_event(rename(RenameMode::From, "/real/from"));
            let mut destinations = ["/real/sub/to", "/alias/to"];
            if alias_first {
                destinations.reverse();
            }
            state.add_event(rename(RenameMode::To, destinations[0]));
            MockTime::advance(Duration::from_millis(10));
            // Expiring an unrelated queue must not finish the pending alias destination.
            assert_eq!(
                state.debounced_events(),
                [DebouncedEvent::new(unrelated, time)]
            );
            state.add_event(rename(RenameMode::To, destinations[1]));
            state.finish_pending_renames();

            assert_eq!(
                state.queues[Path::new("/real/sub/to")].events[0].paths,
                [PathBuf::from("/real/from"), PathBuf::from("/real/sub/to")]
            );
            assert_eq!(
                state.queues[Path::new("/alias/to")].events[0].kind,
                EventKind::Modify(ModifyKind::Name(RenameMode::To))
            );
        }
    }

    #[test]
    fn recursive_mode_uses_recursive_root_for_overlapping_watches() {
        let state = DebounceDataInner {
            queues: BTreeMap::new(),
            vacated: HashMap::default(),
            roots: VecDeque::from([
                WatchRoot {
                    path: PathBuf::from("root"),
                    recursive_mode: RecursiveMode::NonRecursive,
                },
                WatchRoot {
                    path: PathBuf::from("root/nested"),
                    recursive_mode: RecursiveMode::Recursive,
                },
            ]),
            cache: NoCache,
            rename_events: Vec::new(),
            pending_rename_destinations: Vec::new(),
            rescan_event: None,
            errors: Vec::new(),
            timeout: Duration::from_millis(50),
            created: now(),
            waits: false,
        };

        assert_eq!(
            state.recursive_mode(Path::new("root/nested/child")),
            RecursiveMode::Recursive
        );
        assert_eq!(
            state.recursive_mode(Path::new("root/other")),
            RecursiveMode::NonRecursive
        );
    }

    #[test]
    fn errors_keep_user_data_when_delivered() {
        let (tx, rx) = std::sync::mpsc::channel();
        let debouncer =
            new_debouncer_opt::<_, TrackingWatcher, NoCache>(test_config(), tx, NoCache).unwrap();
        {
            let mut data = debouncer.data.inner.lock().unwrap();
            data.add_error(Error::path_not_found().set_user_data(42));
            debouncer.data.changed.notify_all();
        }
        let errors = rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].user_data(), Some(42));
        debouncer.stop();
    }

    #[test]
    fn idle_worker_delivers_events_errors_and_rescans() {
        struct InjectWatcher(Box<dyn notify::EventHandler>);

        impl Watcher for InjectWatcher {
            fn new<F: notify::EventHandler>(handler: F, _: notify::Config) -> notify::Result<Self> {
                Ok(Self(Box::new(handler)))
            }

            fn watch(&mut self, _: &Path, _: RecursiveMode) -> notify::Result<()> {
                Ok(())
            }

            fn unwatch(&mut self, _: &Path) -> notify::Result<()> {
                Ok(())
            }

            fn kind() -> WatcherKind {
                WatcherKind::NullWatcher
            }
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer_opt::<_, InjectWatcher, NoCache>(
            test_config()
                .with_timeout(Duration::from_millis(10))
                .with_tick_rate(Duration::from_millis(2)),
            tx,
            NoCache,
        )
        .unwrap();

        for round in 0..3 {
            // Let the worker return to its idle wait after each delivery.
            std::thread::sleep(Duration::from_millis(20));
            // Ignored events do not leave work for the worker to process.
            debouncer
                .watcher
                .0
                .handle_event(Ok(Event::new(EventKind::Other)));
            match round {
                0 => {
                    let event = Event::new(EventKind::Modify(ModifyKind::Any))
                        .add_path(PathBuf::from("file"));
                    debouncer.watcher.0.handle_event(Ok(event.clone()));
                    let events = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
                    assert_eq!(events.len(), 1);
                    assert_eq!(events[0].event, event);
                }
                1 => {
                    debouncer
                        .watcher
                        .0
                        .handle_event(Err(Error::path_not_found().set_user_data(7)));
                    let errors = rx
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap()
                        .unwrap_err();
                    assert_eq!(errors.len(), 1);
                    assert_eq!(errors[0].user_data(), Some(7));
                }
                _ => {
                    debouncer.watcher.0.handle_event(Ok(
                        Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan)
                    ));
                    let events = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
                    assert_eq!(events.len(), 1);
                    assert!(events[0].need_rescan());
                }
            }
        }
        debouncer.stop();
    }

    #[test]
    fn a_pending_destination_is_delivered_without_a_normal_queue()
    -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let debouncer =
            new_debouncer_opt::<_, TrackingWatcher, NoCache>(test_config(), tx, NoCache)?;
        let path = PathBuf::from("/alias/to");
        {
            let mut data = debouncer.data.inner.lock().unwrap();
            data.pending_rename_destinations.push(DebouncedEvent::new(
                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To)))
                    .add_path(path.clone())
                    .set_tracker(1),
                Instant::now(),
            ));
            debouncer.data.changed.notify_all();
        }
        let events = rx.recv_timeout(Duration::from_secs(5))?.unwrap();
        assert_eq!(events[0].paths, [path]);
        debouncer.stop();
        Ok(())
    }

    const TIMEOUT: Duration = Duration::from_millis(50);

    fn file_state() -> DebounceDataInner<NoCache> {
        MockTime::set_time(Instant::now());
        let mut state = DebounceDataInner::new(NoCache, TIMEOUT);
        state.roots = VecDeque::from([WatchRoot {
            path: PathBuf::from("/watch"),
            recursive_mode: RecursiveMode::Recursive,
        }]);
        state
    }

    fn file_event(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }

    fn created(path: &str) -> Event {
        file_event(EventKind::Create(CreateKind::File), path)
    }

    fn removed(path: &str) -> Event {
        file_event(EventKind::Remove(RemoveKind::File), path)
    }

    /// A removal synthesized by the debouncer.
    fn removed_any(path: &str) -> Event {
        file_event(EventKind::Remove(RemoveKind::Any), path)
    }

    fn renamed(from: &str, to: &str, tracker: usize) -> Event {
        file_event(EventKind::Modify(ModifyKind::Name(RenameMode::Both)), from)
            .add_path(PathBuf::from(to))
            .set_tracker(tracker)
    }

    /// Adds a backend's `From`/`To` pair.
    fn add_rename(state: &mut DebounceDataInner<NoCache>, from: &str, to: &str, tracker: usize) {
        for (mode, path) in [(RenameMode::From, from), (RenameMode::To, to)] {
            state.add_event(
                file_event(EventKind::Modify(ModifyKind::Name(mode)), path).set_tracker(tracker),
            );
        }
    }

    /// Lets every event expire and returns the delivered events without their times.
    fn flush(state: &mut DebounceDataInner<NoCache>) -> Vec<Event> {
        MockTime::advance(TIMEOUT);
        let events = state.debounced_events();
        assert!(state.queues.is_empty(), "{:?}", state.queues);
        assert!(state.vacated.is_empty(), "{:?}", state.vacated);
        events.into_iter().map(|event| event.event).collect()
    }

    /// Blender's backup save must vacate the original path before recreating it.
    /// See <https://github.com/notify-rs/notify/issues/587>.
    #[rstest]
    fn a_file_replaced_through_a_backup_is_created_after_the_backup_rename(
        // The path order of the file and its backup must not decide the event order.
        #[values(("test.blend", "test.blend1"), ("b.blend", "a.blend"))] names: (&str, &str),
        #[values(0, 1)] step: u64,
        #[values(false, true)] backup_exists: bool,
    ) {
        let file = format!("/watch/{}", names.0);
        let backup = format!("/watch/{}", names.1);
        let temp = format!("{file}@");
        let step = Duration::from_millis(step);
        let mut state = file_state();

        state.add_event(created(&temp));
        MockTime::advance(step);
        if backup_exists {
            state.add_event(removed(&backup));
            MockTime::advance(step);
        }
        add_rename(&mut state, &file, &backup, 1);
        MockTime::advance(step);
        add_rename(&mut state, &temp, &file, 2);

        let mut expected = vec![renamed(&file, &backup, 1), created(&file)];
        if backup_exists {
            expected.insert(0, removed_any(&backup));
        }
        assert_eq!(flush(&mut state), expected);
    }

    #[test]
    fn a_create_waits_for_the_rename_that_vacated_its_path() {
        let mut state = file_state();
        let start = now();
        state.add_event(created("/watch/test.blend@"));
        MockTime::advance(Duration::from_millis(40));
        state.add_event(removed("/watch/test.blend1"));
        add_rename(&mut state, "/watch/test.blend", "/watch/test.blend1", 1);
        add_rename(&mut state, "/watch/test.blend@", "/watch/test.blend", 2);

        // The create expires 40 ms before the rename that makes room for its path.
        for delay in [10, 39] {
            MockTime::advance(Duration::from_millis(delay));
            assert_eq!(state.debounced_events(), []);
        }
        MockTime::advance(Duration::from_millis(1));
        let renamed_at = start + Duration::from_millis(40);
        assert_eq!(
            state.debounced_events(),
            [
                DebouncedEvent::new(removed_any("/watch/test.blend1"), renamed_at),
                DebouncedEvent::new(
                    renamed("/watch/test.blend", "/watch/test.blend1", 1),
                    renamed_at
                ),
                DebouncedEvent::new(created("/watch/test.blend"), start),
            ]
        );
        assert!(state.queues.is_empty());
    }

    #[rstest]
    fn removing_a_renamed_file_reports_the_removal_of_its_source(
        #[values(("a", "b"), ("b", "a"))] names: (&str, &str),
    ) {
        let source = format!("/watch/{}", names.0);
        let destination = format!("/watch/{}", names.1);
        let mut state = file_state();
        let start = now();

        add_rename(&mut state, &source, &destination, 1);
        MockTime::advance(Duration::from_millis(1));
        state.add_event(removed(&destination));

        MockTime::advance(TIMEOUT);
        assert_eq!(
            state.debounced_events(),
            [
                DebouncedEvent::new(removed_any(&source), start),
                DebouncedEvent::new(removed(&destination), start + Duration::from_millis(1)),
            ]
        );
        assert!(state.queues.is_empty());
    }

    /// Synthesized removals belong to the watch covering the removed path.
    #[rstest]
    // The file moves between two watches, and then to a third one.
    #[case::moved_to_another_watch(&[("/watch/a", 1), ("/watch/b", 2)])]
    #[case::moved_twice(&[("/watch/a", 1), ("/watch/b", 2), ("/watch/c", 3)])]
    #[case::moved_within_a_watch(&[("/watch/a", 7), ("/watch/b", 7)])]
    fn the_removal_of_a_rename_source_has_the_user_data_of_its_watch(
        #[case] moves: &[(&str, u64)],
    ) {
        let mut state = file_state();
        let renamed = |mode, (path, user_data): (&str, u64), tracker| {
            file_event(EventKind::Modify(ModifyKind::Name(mode)), path)
                .set_tracker(tracker)
                .set_user_data(user_data)
        };
        for (tracker, pair) in moves.windows(2).enumerate() {
            state.add_event(renamed(RenameMode::From, pair[0], tracker));
            state.add_event(renamed(RenameMode::To, pair[1], tracker));
        }
        let (source, source_data) = moves[0];
        let (destination, destination_data) = moves[moves.len() - 1];
        MockTime::advance(Duration::from_millis(1));
        state.add_event(removed(destination).set_user_data(destination_data));

        assert_eq!(
            flush(&mut state),
            [
                removed_any(source).set_user_data(source_data),
                removed(destination).set_user_data(destination_data),
            ]
        );
    }

    /// Builds `c -> b -> a -> z`, with the last rename 40 ms newer than the others.
    fn add_waiting_renames(state: &mut DebounceDataInner<NoCache>) {
        let moved = |mode, path: &str, tracker, user_data| {
            file_event(EventKind::Modify(ModifyKind::Name(mode)), path)
                .set_tracker(tracker)
                .set_user_data(user_data)
        };
        state.add_event(moved(RenameMode::From, "/watch/c", 1, 3));
        state.add_event(moved(RenameMode::To, "/watch/c@", 1, 3));
        state.add_event(moved(RenameMode::From, "/watch/b", 2, 2));
        state.add_event(moved(RenameMode::To, "/watch/b@", 2, 2));
        MockTime::advance(Duration::from_millis(40));
        add_rename(state, "/watch/a", "/watch/z", 3);
        state.add_event(moved(RenameMode::From, "/watch/b@", 4, 2));
        state.add_event(moved(RenameMode::To, "/watch/a", 4, 1));
        state.add_event(moved(RenameMode::From, "/watch/c@", 5, 3));
        state.add_event(moved(RenameMode::To, "/watch/b", 5, 2));
    }

    #[test]
    fn the_user_data_of_a_rename_source_outlasts_a_wait() {
        let mut state = file_state();
        add_waiting_renames(&mut state);

        // The older renames must wait for `a -> z`.
        MockTime::advance(Duration::from_millis(20));
        assert_eq!(state.debounced_events(), []);
        testing::assert_vacated_paths(&state, &"after the wait");
        state.add_event(removed("/watch/b").set_user_data(2));

        let events = flush(&mut state);
        assert!(
            events.contains(&removed_any("/watch/c").set_user_data(3)),
            "{events:?}"
        );
    }

    #[test]
    fn the_removal_of_a_replaced_file_has_the_user_data_of_its_watch() {
        let mut state = file_state();
        let renamed = |mode, path: &str, tracker, user_data| {
            file_event(EventKind::Modify(ModifyKind::Name(mode)), path)
                .set_tracker(tracker)
                .set_user_data(user_data)
        };
        // `a` replaces `b`, which belongs to another watch and was modified before.
        let content = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        state.add_event(file_event(content, "/watch/b").set_user_data(2));
        state.add_event(renamed(RenameMode::From, "/watch/a", 1, 1));
        state.add_event(renamed(RenameMode::To, "/watch/b", 1, 2));
        assert_eq!(
            state.queues[Path::new("/watch/b")].events[0].event,
            removed_any("/watch/b")
                .set_user_data(2)
                .set_info("override")
        );

        // `c` replaces `b` again, before the rename of `a` is delivered.
        state.add_event(renamed(RenameMode::From, "/watch/c", 2, 3));
        state.add_event(renamed(RenameMode::To, "/watch/b", 2, 2));
        assert_eq!(
            flush(&mut state),
            [
                removed_any("/watch/a").set_user_data(1),
                removed_any("/watch/b").set_user_data(2),
                renamed(RenameMode::Both, "/watch/c", 2, 2).add_path(PathBuf::from("/watch/b")),
            ]
        );
    }

    #[test]
    fn removing_a_renamed_file_keeps_the_events_that_follow_for_its_source() {
        let mut state = file_state();
        let start = now();
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        MockTime::advance(Duration::from_millis(1));
        state.add_event(created("/watch/a"));
        MockTime::advance(Duration::from_millis(1));
        state.add_event(removed("/watch/b"));

        assert_eq!(
            state.queues[Path::new("/watch/a")].events,
            [
                DebouncedEvent::new(removed_any("/watch/a"), start),
                DebouncedEvent::new(created("/watch/a"), start + Duration::from_millis(1)),
            ]
        );
        assert_eq!(
            flush(&mut state),
            [
                removed_any("/watch/a"),
                created("/watch/a"),
                removed("/watch/b")
            ]
        );
    }

    #[test]
    fn removing_a_renamed_file_does_not_repeat_the_removal_of_its_source() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        state.add_event(created("/watch/a"));
        state.add_event(removed("/watch/a"));
        state.add_event(removed("/watch/b"));

        assert_eq!(
            flush(&mut state),
            [removed("/watch/a"), removed("/watch/b")]
        );
    }

    #[test]
    fn removing_a_file_that_was_renamed_twice_reports_the_removal_of_its_first_source() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        add_rename(&mut state, "/watch/b", "/watch/c", 2);
        MockTime::advance(Duration::from_millis(1));
        state.add_event(removed("/watch/c"));

        assert_eq!(
            flush(&mut state),
            [removed_any("/watch/a"), removed("/watch/c")]
        );
    }

    #[test]
    fn removing_a_file_reports_only_the_paths_of_its_undelivered_renames() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        assert_eq!(flush(&mut state), [renamed("/watch/a", "/watch/b", 1)]);

        // The rename to `b` was delivered, so `a` is known to be gone.
        state.add_event(removed("/watch/b"));
        assert_eq!(flush(&mut state), [removed("/watch/b")]);

        add_rename(&mut state, "/watch/c", "/watch/d", 2);
        assert_eq!(flush(&mut state), [renamed("/watch/c", "/watch/d", 2)]);
        add_rename(&mut state, "/watch/d", "/watch/e", 3);
        MockTime::advance(Duration::from_millis(1));
        state.add_event(removed("/watch/e"));
        assert_eq!(
            flush(&mut state),
            [removed_any("/watch/d"), removed("/watch/e")]
        );
    }

    #[test]
    fn removing_a_directory_reports_the_removal_of_the_sources_of_files_moved_into_it() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/out/a", "/watch/dir/sub/b", 1);
        add_rename(&mut state, "/watch/dir/x", "/watch/dir/y", 2);
        add_rename(&mut state, "/watch/dir/z", "/watch/out/z", 3);
        MockTime::advance(Duration::from_millis(1));
        let removal = file_event(EventKind::Remove(RemoveKind::Folder), "/watch/dir");
        state.add_event(removal.clone());

        // The removal of the directory covers `/watch/dir/x`.
        assert_eq!(
            flush(&mut state),
            [
                removed_any("/watch/out/a"),
                renamed("/watch/dir/z", "/watch/out/z", 3),
                removal
            ]
        );
    }

    #[test]
    fn replacing_a_renamed_file_reports_the_removal_of_its_source() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        MockTime::advance(Duration::from_millis(1));
        add_rename(&mut state, "/watch/c", "/watch/b", 2);

        assert_eq!(
            flush(&mut state),
            [
                removed_any("/watch/a"),
                removed_any("/watch/b").set_info("override"),
                renamed("/watch/c", "/watch/b", 2),
            ]
        );
    }

    #[test]
    fn creating_a_renamed_file_again_reports_the_removal_of_its_source() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        // The file at `b` is replaced without a removal, and moves on.
        state.add_event(created("/watch/b"));
        add_rename(&mut state, "/watch/b", "/watch/c", 2);

        assert_eq!(
            flush(&mut state),
            [removed_any("/watch/a"), created("/watch/c")]
        );
    }

    #[rstest]
    fn a_file_created_at_the_source_of_a_rename_follows_the_rename(
        #[values(("a", "b"), ("b", "a"))] names: (&str, &str),
        #[values(0, 1)] step: u64,
    ) {
        let source = format!("/watch/{}", names.0);
        let destination = format!("/watch/{}", names.1);
        let mut state = file_state();
        add_rename(&mut state, &source, &destination, 1);
        MockTime::advance(Duration::from_millis(step));
        state.add_event(created(&source));

        assert_eq!(
            flush(&mut state),
            [renamed(&source, &destination, 1), created(&source)]
        );
    }

    #[test]
    fn a_file_created_at_the_first_source_of_two_renames_follows_them() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        add_rename(&mut state, "/watch/b", "/watch/c", 2);
        assert_eq!(
            state.vacated.keys().map(AsRef::as_ref).collect::<Vec<_>>(),
            [Path::new("/watch/a")]
        );
        state.add_event(created("/watch/a"));
        state.add_event(created("/watch/b"));

        assert_eq!(
            flush(&mut state),
            [
                created("/watch/b"),
                renamed("/watch/a", "/watch/c", 2),
                created("/watch/a"),
            ]
        );
    }

    #[test]
    fn a_removal_at_the_source_of_a_rename_follows_the_rename() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        state.add_event(created("/watch/a"));
        state.add_event(removed("/watch/a"));

        assert_eq!(
            flush(&mut state),
            [renamed("/watch/a", "/watch/b", 1), removed("/watch/a")]
        );
    }

    #[test]
    fn events_wait_until_their_rename_expires() {
        let mut state = file_state();
        let start = now();
        let after = |millis| start + Duration::from_millis(millis);
        // The file that is created at 10 ms moves to `b` at 20 ms, after `b` moved to `c`.
        MockTime::set_time(after(10));
        state.add_event(created("/watch/a"));
        MockTime::set_time(after(20));
        add_rename(&mut state, "/watch/b", "/watch/c", 1);
        add_rename(&mut state, "/watch/a", "/watch/b", 2);
        MockTime::set_time(after(30));
        state.add_event(created("/watch/c.new"));

        let mut delivered = Vec::new();
        for millis in [59, 60, 69, 70, 79, 80] {
            MockTime::set_time(after(millis));
            delivered.push((millis, state.debounced_events()));
        }
        assert_eq!(
            delivered,
            [
                (59, vec![]),
                // The create at `b` is 60 ms old, but its path is vacated at 70 ms.
                (60, vec![]),
                (69, vec![]),
                (
                    70,
                    vec![
                        DebouncedEvent::new(renamed("/watch/b", "/watch/c", 1), after(20)),
                        DebouncedEvent::new(created("/watch/b"), after(10)),
                    ]
                ),
                (79, vec![]),
                (
                    80,
                    vec![DebouncedEvent::new(created("/watch/c.new"), after(30))]
                ),
            ]
        );
        assert!(state.queues.is_empty());
        assert!(state.vacated.is_empty());
    }

    #[test]
    fn events_wait_for_a_rename_that_waits_itself() {
        let mut state = file_state();
        let start = now();
        let after = |millis| start + Duration::from_millis(millis);
        // `o` moves to `p` and on to `h`, which moved to `z` just before.
        add_rename(&mut state, "/watch/o", "/watch/p", 1);
        MockTime::set_time(after(1));
        state.add_event(created("/watch/o"));
        MockTime::set_time(after(20));
        add_rename(&mut state, "/watch/h", "/watch/z", 2);
        add_rename(&mut state, "/watch/p", "/watch/h", 3);

        // The rename to `h` and the create at `o` are expired, but follow the rename to `z`.
        for millis in [51, 69] {
            MockTime::set_time(after(millis));
            assert_eq!(state.debounced_events(), []);
            testing::assert_vacated_paths(&state, &millis);
            assert_eq!(
                state.queues.keys().collect::<Vec<_>>(),
                ["/watch/h", "/watch/o", "/watch/z"].map(Path::new)
            );
        }
        MockTime::set_time(after(70));
        assert_eq!(
            state.debounced_events(),
            [
                DebouncedEvent::new(renamed("/watch/h", "/watch/z", 2), after(20)),
                DebouncedEvent::new(renamed("/watch/o", "/watch/h", 3), start),
                DebouncedEvent::new(created("/watch/o"), after(1)),
            ]
        );
        assert!(state.queues.is_empty());
        assert!(state.vacated.is_empty());
    }

    #[test]
    fn waiting_events_return_to_the_front_of_their_queue() {
        use notify::event::{AccessKind, AccessMode};

        let mut state = file_state();
        let start = now();
        let after = |millis| start + Duration::from_millis(millis);
        let access = |mode| file_event(EventKind::Access(mode), "/watch/c");
        let closed = access(AccessKind::Close(AccessMode::Write));
        let opened = access(AccessKind::Open(AccessMode::Any));
        add_waiting_renames(&mut state);
        // A new file at `c` follows the rename from `c`.
        MockTime::set_time(after(1));
        state.add_event(created("/watch/c"));
        MockTime::set_time(after(5));
        state.add_event(closed.clone());
        MockTime::set_time(after(55));
        state.add_event(opened.clone());

        // The rename from `c` is expired, but it waits for the renames from `b` and from `a`.
        // The expired events for `c` and for `b` return to their queues.
        MockTime::set_time(after(60));
        assert_eq!(state.debounced_events(), []);
        testing::assert_vacated_paths(&state, &"after the wait");
        assert_eq!(
            state.queues[Path::new("/watch/c")].events,
            [
                DebouncedEvent::new(created("/watch/c"), after(1)),
                DebouncedEvent::new(closed.clone(), after(5)),
                DebouncedEvent::new(opened.clone(), after(55)),
            ]
        );
        assert_eq!(
            state.queues.keys().collect::<Vec<_>>(),
            ["/watch/a", "/watch/b", "/watch/c", "/watch/z"].map(Path::new)
        );

        MockTime::set_time(after(90));
        let events: Vec<_> = state
            .debounced_events()
            .into_iter()
            .map(|event| (event.event.kind, event.event.paths, event.time))
            .collect();
        let path = |path: &str| PathBuf::from(path);
        assert_eq!(
            events,
            [
                (RENAME, vec![path("/watch/a"), path("/watch/z")], after(40)),
                (RENAME, vec![path("/watch/b"), path("/watch/a")], start),
                (RENAME, vec![path("/watch/c"), path("/watch/b")], start),
                (created("/watch/c").kind, vec![path("/watch/c")], after(1)),
                (closed.kind, vec![path("/watch/c")], after(5)),
            ]
        );
        MockTime::set_time(after(105));
        assert_eq!(
            state.debounced_events(),
            [DebouncedEvent::new(opened, after(55))]
        );
        assert!(state.queues.is_empty());
        assert!(state.vacated.is_empty());
    }

    /// Duplicate kinds expose unnecessary draining: `take_expired` would deduplicate them.
    #[test]
    fn events_that_wait_for_an_unexpired_rename_stay_in_their_queue() {
        use notify::event::{AccessKind, AccessMode};

        let mut state = file_state();
        let start = now();
        let after = |millis| start + Duration::from_millis(millis);
        let closed = |path| {
            file_event(
                EventKind::Access(AccessKind::Close(AccessMode::Write)),
                path,
            )
        };
        // The file that is written as `a@` replaces `a`, which moves to `b`.
        state.add_event(created("/watch/a@"));
        for millis in [5, 10] {
            MockTime::set_time(after(millis));
            state.add_event(closed("/watch/a@"));
        }
        MockTime::set_time(after(40));
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        add_rename(&mut state, "/watch/a@", "/watch/a", 2);
        let queues = state.queues.clone();

        for millis in [60, 75, 89] {
            MockTime::set_time(after(millis));
            assert_eq!(state.debounced_events(), []);
            assert_eq!(state.queues, queues);
            testing::assert_vacated_paths(&state, &millis);
        }
        MockTime::set_time(after(90));
        assert_eq!(
            state.debounced_events(),
            [
                DebouncedEvent::new(renamed("/watch/a", "/watch/b", 1), after(40)),
                DebouncedEvent::new(created("/watch/a"), start),
                DebouncedEvent::new(closed("/watch/a"), after(10)),
            ]
        );
        assert!(state.queues.is_empty());
        assert!(state.vacated.is_empty());
    }

    #[test]
    fn events_that_returned_to_their_queue_stay_there() {
        use notify::event::{AccessKind, AccessMode};

        let mut state = file_state();
        let start = now();
        let after = |millis| start + Duration::from_millis(millis);
        let opened = EventKind::Access(AccessKind::Open(AccessMode::Any));
        let closed = EventKind::Access(AccessKind::Close(AccessMode::Write));
        add_waiting_renames(&mut state);
        MockTime::set_time(after(1));
        state.add_event(created("/watch/c"));
        for (millis, kind) in [(2, opened), (3, closed)] {
            MockTime::set_time(after(millis));
            state.add_event(file_event(kind, "/watch/b"));
            state.add_event(file_event(kind, "/watch/c"));
        }

        // The renames from `c` and from `b` are expired, but wait for the rename from `a`.
        MockTime::set_time(after(60));
        assert_eq!(state.debounced_events(), []);
        MockTime::set_time(after(61));
        state.add_event(file_event(closed, "/watch/b"));
        state.add_event(file_event(closed, "/watch/c"));
        let queues = state.queues.clone();
        for millis in [62, 75, 89] {
            MockTime::set_time(after(millis));
            assert_eq!(state.debounced_events(), []);
            assert_eq!(state.queues, queues);
            testing::assert_vacated_paths(&state, &millis);
        }

        let mut delivered = |millis| {
            MockTime::set_time(after(millis));
            let events = state.debounced_events().into_iter();
            let event =
                |event: DebouncedEvent| (event.kind, event.paths.last().cloned(), event.time);
            events.map(event).collect::<Vec<_>>()
        };
        let path = |path: &str| Some(PathBuf::from(path));
        assert_eq!(
            delivered(90),
            [
                (RENAME, path("/watch/z"), after(40)),
                (RENAME, path("/watch/a"), start),
                (RENAME, path("/watch/b"), start),
                (created("/watch/c").kind, path("/watch/c"), after(1)),
                (opened, path("/watch/b"), after(2)),
                (opened, path("/watch/c"), after(2)),
            ]
        );
        assert_eq!(
            delivered(111),
            [
                (closed, path("/watch/b"), after(61)),
                (closed, path("/watch/c"), after(61)),
            ]
        );
        assert!(state.queues.is_empty());
        assert!(state.vacated.is_empty());
    }

    #[test]
    fn events_that_returned_to_their_queue_are_delivered_with_their_rename() {
        let mut state = file_state();
        let start = now();
        let after = |millis| start + Duration::from_millis(millis);
        // A new file at `x` follows the rename of `x`, which continues to `a` at 40 ms.
        add_rename(&mut state, "/watch/x", "/watch/tmp", 1);
        state.add_event(created("/watch/x"));
        state.add_event(created("/watch/c.tmp"));
        MockTime::set_time(after(40));
        add_rename(&mut state, "/watch/a", "/watch/b", 2);
        add_rename(&mut state, "/watch/tmp", "/watch/a", 3);
        state.add_event(removed("/watch/b"));
        // The rename of `c` at 50 ms is unrelated to `x`.
        MockTime::set_time(after(50));
        add_rename(&mut state, "/watch/c", "/watch/d", 4);
        add_rename(&mut state, "/watch/c.tmp", "/watch/c", 5);

        // The rename to `a` is expired, but follows the removal of `a` at 40 ms.
        MockTime::set_time(after(60));
        assert_eq!(state.debounced_events(), []);
        testing::assert_vacated_paths(&state, &60);
        MockTime::set_time(after(90));
        assert_eq!(
            state.debounced_events(),
            [
                DebouncedEvent::new(removed_any("/watch/a"), after(40)),
                DebouncedEvent::new(renamed("/watch/x", "/watch/a", 3), start),
                DebouncedEvent::new(created("/watch/x"), start),
                DebouncedEvent::new(removed("/watch/b"), after(40)),
            ]
        );
        testing::assert_vacated_paths(&state, &90);
        MockTime::set_time(after(91));
        assert_eq!(state.debounced_events(), []);
        MockTime::set_time(after(100));
        assert_eq!(
            state.debounced_events(),
            [
                DebouncedEvent::new(renamed("/watch/c", "/watch/d", 4), after(50)),
                DebouncedEvent::new(created("/watch/c"), start),
            ]
        );
        assert!(state.queues.is_empty());
        assert!(state.vacated.is_empty());
    }

    #[test]
    fn unexpired_events_stay_when_the_rename_that_they_follow_is_delivered() {
        let mut state = file_state();
        let start = now();
        let after = |millis| start + Duration::from_millis(millis);
        // A new file at `x` follows the rename of `x`, which continues to `a` at 40 ms.
        add_rename(&mut state, "/watch/x", "/watch/tmp", 1);
        state.add_event(created("/watch/x"));
        MockTime::set_time(after(40));
        add_rename(&mut state, "/watch/a", "/watch/b", 2);
        add_rename(&mut state, "/watch/tmp", "/watch/a", 3);
        state.add_event(removed("/watch/b"));

        // The create at `x` returns to its queue, and is replaced by a removal.
        MockTime::set_time(after(60));
        assert_eq!(state.debounced_events(), []);
        MockTime::set_time(after(70));
        state.add_event(removed("/watch/x"));

        MockTime::set_time(after(90));
        assert_eq!(
            state.debounced_events(),
            [
                DebouncedEvent::new(removed_any("/watch/a"), after(40)),
                DebouncedEvent::new(renamed("/watch/x", "/watch/a", 3), start),
                DebouncedEvent::new(removed("/watch/b"), after(40)),
            ]
        );
        testing::assert_vacated_paths(&state, &90);
        MockTime::set_time(after(120));
        assert_eq!(
            state.debounced_events(),
            [DebouncedEvent::new(removed("/watch/x"), after(70))]
        );
        assert!(state.queues.is_empty());
        assert!(state.vacated.is_empty());
    }

    /// Continuing a deferred rename can turn its dependency chain into a cycle.
    #[test]
    fn renames_that_come_to_wait_for_each_other_are_delivered() {
        let mut state = file_state();
        let start = now();
        let after = |millis| start + Duration::from_millis(millis);
        add_rename(&mut state, "/watch/a", "/watch/a.tmp", 1);
        add_rename(&mut state, "/watch/b", "/watch/b.tmp", 2);
        MockTime::set_time(after(40));
        add_rename(&mut state, "/watch/c", "/watch/z", 3);
        add_rename(&mut state, "/watch/a.tmp", "/watch/c", 4);
        add_rename(&mut state, "/watch/b.tmp", "/watch/a", 5);

        // `b -> a` waits for `a -> c`, which waits for `c -> z`.
        MockTime::set_time(after(60));
        assert_eq!(state.debounced_events(), []);
        // The rename from `a` continues to `b`, and now follows the rename from `b`.
        MockTime::set_time(after(61));
        add_rename(&mut state, "/watch/c", "/watch/b", 6);
        testing::assert_vacated_paths(&state, &61);

        MockTime::set_time(after(90));
        assert_eq!(
            state.debounced_events(),
            [
                DebouncedEvent::new(renamed("/watch/c", "/watch/z", 3), after(40)),
                DebouncedEvent::new(renamed("/watch/b", "/watch/a", 5), start),
                DebouncedEvent::new(renamed("/watch/a", "/watch/b", 6), start),
            ]
        );
        assert!(state.queues.is_empty());
        assert!(state.vacated.is_empty());
    }

    #[test]
    fn a_vacated_path_is_small() {
        // See `Vacated`.
        assert_eq!(size_of::<Vacated>(), 24);
    }

    #[rstest]
    fn rotated_files_are_renamed_from_the_last_to_the_first(#[values(0, 1)] step: u64) {
        let mut state = file_state();
        for (tracker, index) in (0..3).rev().enumerate() {
            add_rename(
                &mut state,
                &format!("/watch/log.{index}"),
                &format!("/watch/log.{}", index + 1),
                tracker,
            );
            MockTime::advance(Duration::from_millis(step));
        }
        state.add_event(created("/watch/log.0"));

        assert_eq!(
            flush(&mut state),
            [
                renamed("/watch/log.2", "/watch/log.3", 0),
                renamed("/watch/log.1", "/watch/log.2", 1),
                renamed("/watch/log.0", "/watch/log.1", 2),
                created("/watch/log.0"),
            ]
        );
    }

    /// A swap creates a cycle between the two pending renames.
    #[test]
    fn renames_that_wait_for_each_other_are_delivered() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/a", "/watch/tmp", 1);
        MockTime::advance(Duration::from_millis(1));
        add_rename(&mut state, "/watch/b", "/watch/a", 2);
        add_rename(&mut state, "/watch/tmp", "/watch/b", 3);
        state.add_event(created("/watch/tmp"));

        // The rename to `b` waits for the rename to `a`, which expires 1 ms later.
        MockTime::advance(TIMEOUT - Duration::from_millis(1));
        assert_eq!(state.debounced_events(), []);
        MockTime::advance(Duration::from_millis(1));
        let events: Vec<_> = state
            .debounced_events()
            .into_iter()
            .map(|event| event.event)
            .collect();
        assert_eq!(
            events,
            [
                created("/watch/tmp"),
                renamed("/watch/a", "/watch/b", 3),
                renamed("/watch/b", "/watch/a", 2),
            ]
        );
        assert!(state.queues.is_empty());
        assert!(state.vacated.is_empty());
    }

    #[test]
    fn a_file_that_is_renamed_back_is_delivered() {
        let mut state = file_state();
        add_rename(&mut state, "/watch/a", "/watch/b", 1);
        add_rename(&mut state, "/watch/b", "/watch/a", 2);

        assert_eq!(flush(&mut state), [renamed("/watch/a", "/watch/a", 2)]);
    }

    #[test]
    fn a_file_saved_twice_through_a_backup_replaces_the_file_and_the_backup() {
        let mut state = file_state();
        for save in 0..2 {
            state.add_event(created("/watch/test.blend@"));
            state.add_event(removed("/watch/test.blend1"));
            add_rename(
                &mut state,
                "/watch/test.blend",
                "/watch/test.blend1",
                save * 2,
            );
            add_rename(
                &mut state,
                "/watch/test.blend@",
                "/watch/test.blend",
                save * 2 + 1,
            );
        }

        assert_eq!(
            flush(&mut state),
            [
                removed_any("/watch/test.blend"),
                created("/watch/test.blend"),
                removed_any("/watch/test.blend1"),
                created("/watch/test.blend1"),
            ]
        );
    }

    #[derive(Debug, Clone, Copy)]
    enum Op {
        Create(&'static str),
        Remove(&'static str),
        Rename(&'static str, &'static str),
    }

    /// Applies emitted events to the consumer's file set.
    fn apply(files: &mut std::collections::BTreeSet<PathBuf>, event: &Event) {
        match event.kind {
            EventKind::Create(_) => {
                files.insert(event.paths[0].clone());
            }
            EventKind::Remove(_) => files.retain(|file| !file.starts_with(&event.paths[0])),
            RENAME => {
                assert!(files.remove(&event.paths[0]), "unknown source: {event:?}");
                files.insert(event.paths[1].clone());
            }
            _ => {}
        }
    }

    /// Replaying all emitted events must reconstruct the final file set.
    #[rstest]
    #[case::backup_save(
        &["test.blend", "test.blend1"],
        &[
            Op::Create("test.blend@"),
            Op::Remove("test.blend1"),
            Op::Rename("test.blend", "test.blend1"),
            Op::Rename("test.blend@", "test.blend"),
        ],
    )]
    #[case::first_backup_save(
        &["b.blend"],
        &[
            Op::Create("b.blend@"),
            Op::Rename("b.blend", "a.blend"),
            Op::Rename("b.blend@", "b.blend"),
        ],
    )]
    #[case::two_backup_saves(
        &["test.blend", "test.blend1"],
        &[
            Op::Create("test.blend@"),
            Op::Remove("test.blend1"),
            Op::Rename("test.blend", "test.blend1"),
            Op::Rename("test.blend@", "test.blend"),
            Op::Create("test.blend@"),
            Op::Remove("test.blend1"),
            Op::Rename("test.blend", "test.blend1"),
            Op::Rename("test.blend@", "test.blend"),
        ],
    )]
    #[case::atomic_save(&["file"], &[Op::Create("file.tmp"), Op::Rename("file.tmp", "file")])]
    #[case::rename_and_remove(&["a"], &[Op::Rename("a", "b"), Op::Remove("b")])]
    #[case::rename_and_remove_reversed(&["b"], &[Op::Rename("b", "a"), Op::Remove("a")])]
    #[case::renames_and_remove(
        &["a"],
        &[Op::Rename("a", "b"), Op::Rename("b", "c"), Op::Remove("c")],
    )]
    #[case::recreated_source(&["a"], &[Op::Rename("a", "b"), Op::Create("a")])]
    #[case::recreated_source_reversed(&["b"], &[Op::Rename("b", "a"), Op::Create("b")])]
    #[case::recreated_source_and_remove(
        &["a"],
        &[Op::Rename("a", "b"), Op::Create("a"), Op::Remove("b")],
    )]
    #[case::recreated_and_removed_source(
        &["a"],
        &[Op::Rename("a", "b"), Op::Create("a"), Op::Remove("a")],
    )]
    #[case::recreated_and_removed_source_and_remove(
        &["a"],
        &[Op::Rename("a", "b"), Op::Create("a"), Op::Remove("a"), Op::Remove("b")],
    )]
    #[case::recreated_and_renamed_source(
        &["a"],
        &[Op::Rename("a", "b"), Op::Create("a"), Op::Rename("a", "c")],
    )]
    #[case::replaced_destination(&["a", "c"], &[Op::Rename("a", "b"), Op::Rename("c", "b")])]
    #[case::reused_source_and_remove(
        &["a", "c"],
        &[Op::Rename("a", "b"), Op::Rename("c", "a"), Op::Remove("b")],
    )]
    #[case::rotation(
        &["log", "log.1", "log.2"],
        &[
            Op::Rename("log.2", "log.3"),
            Op::Rename("log.1", "log.2"),
            Op::Rename("log", "log.1"),
            Op::Create("log"),
        ],
    )]
    #[case::renamed_back(&["a"], &[Op::Rename("a", "b"), Op::Rename("b", "a")])]
    #[case::removed_directory(
        &["out/a", "dir/x"],
        &[Op::Rename("out/a", "dir/b"), Op::Rename("dir/x", "dir/y"), Op::Remove("dir")],
    )]
    fn delivered_events_lead_to_the_existing_files(#[case] files: &[&str], #[case] ops: &[Op]) {
        let path = |name: &str| PathBuf::from(format!("/watch/{name}"));
        // Pause evenly, or once for about the timeout, so that only the earlier events expire.
        let mut schedules: Vec<Vec<u64>> = [0, 25, 49, 50]
            .into_iter()
            .map(|pause| vec![pause; ops.len()])
            .collect();
        for index in 0..ops.len() {
            for pause in [49, 50] {
                let mut pauses = vec![0; ops.len()];
                pauses[index] = pause;
                schedules.push(pauses);
            }
        }

        for pauses in schedules {
            let mut state = file_state();
            let mut existing: std::collections::BTreeSet<_> =
                files.iter().map(|name| path(name)).collect();
            let mut known = existing.clone();
            for (index, (op, pause)) in ops.iter().zip(&pauses).enumerate() {
                match *op {
                    Op::Create(name) => {
                        state.add_event(created(&format!("/watch/{name}")));
                        existing.insert(path(name));
                    }
                    Op::Remove(name) => {
                        state.add_event(removed(&format!("/watch/{name}")));
                        existing.retain(|file| !file.starts_with(path(name)));
                    }
                    Op::Rename(from, to) => {
                        add_rename(
                            &mut state,
                            &format!("/watch/{from}"),
                            &format!("/watch/{to}"),
                            index,
                        );
                        assert!(existing.remove(&path(from)));
                        existing.insert(path(to));
                    }
                }
                MockTime::advance(Duration::from_millis(*pause));
                for event in state.debounced_events() {
                    apply(&mut known, &event);
                }
            }
            for event in flush(&mut state) {
                apply(&mut known, &event);
            }
            assert_eq!(known, existing, "pauses: {pauses:?}");
        }
    }

    #[test]
    fn random_operations_leave_no_events_or_vacated_paths_behind() {
        let names = ["a", "b", "c", "d"];
        // A linear congruential generator keeps the operations reproducible.
        let mut seed = 1_u64;
        let mut next = move |bound: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % bound
        };
        for _ in 0..5000 {
            let mut state = file_state();
            let mut exists: Vec<bool> = names.iter().map(|_| next(2) == 0).collect();
            let mut steps = vec![format!("{exists:?}")];
            for tracker in 0..2 + next(9) as usize {
                let (first, second) = (next(4) as usize, next(4) as usize);
                let path = format!("/watch/{}", names[first]);
                let operation = next(4);
                match operation {
                    0 if !exists[first] => state.add_event(created(&path)),
                    1 if exists[first] => state.add_event(removed(&path)),
                    2 if exists[first] && first != second => {
                        add_rename(
                            &mut state,
                            &path,
                            &format!("/watch/{}", names[second]),
                            tracker,
                        );
                        exists[second] = true;
                    }
                    3 if exists[first] => {
                        let kind = EventKind::Modify(ModifyKind::Data(DataChange::Content));
                        state.add_event(file_event(kind, &path));
                    }
                    _ => continue,
                }
                exists[first] = operation == 0 || operation == 3;
                let pause = [0, 0, 0, 10, 25, 50][next(6) as usize];
                MockTime::advance(Duration::from_millis(pause));
                let tick = next(2) == 0;
                if tick {
                    state.debounced_events();
                }
                steps.push(format!("{operation} {first} {second} +{pause} {tick}"));
                testing::assert_vacated_paths(&state, &steps);
            }
            flush(&mut state);
        }
    }

    #[test]
    fn sort_events_ties_by_path() {
        MockTime::set_time(Instant::now());
        let mut state = DebounceDataInner::new(NoCache, Duration::ZERO);
        for path in ["/watch/b", "/watch/a"] {
            state.add_event(Event::new(EventKind::Any).add_path(PathBuf::from(path)));
        }

        let paths = state
            .debounced_events()
            .into_iter()
            .map(|event| event.paths[0].clone())
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            vec![PathBuf::from("/watch/a"), PathBuf::from("/watch/b")]
        );
    }

    #[test]
    fn integration() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;

        // set up the watcher
        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(Duration::from_millis(10), None, tx)?;
        debouncer.watch(dir.path(), RecursiveMode::Recursive)?;

        // create a new file
        let file_path = dir.path().join("file.txt");
        fs::write(&file_path, b"Lorem ipsum")?;

        println!("waiting for event at {}", file_path.display());

        // wait for up to 10 seconds for the create event, ignore all other events
        let deadline = Instant::now() + Duration::from_secs(10);
        while deadline > Instant::now() {
            let events = rx
                .recv_timeout(deadline - Instant::now())
                .expect("did not receive expected event")
                .expect("received an error");

            for event in events {
                if event.event.paths == vec![file_path.clone()]
                    || event.event.paths == vec![file_path.canonicalize()?]
                {
                    return Ok(());
                }

                println!("unexpected event: {event:?}");
            }
        }

        panic!("did not receive expected event");
    }

    /// Regression test: on macOS, FSEvents reports file deletions as a burst
    /// of `Create(File)` + `Modify(Data)` + `Remove(File)`.  The debouncer
    /// must not suppress the `Remove` event, even though a prior `Create`
    /// for the same path exists in the queue.
    ///
    /// Without the fix, `push_remove_event` would see `was_created() == true`
    /// and cancel the entire queue, swallowing the removal.
    #[test]
    #[cfg(all(target_os = "macos", feature = "macos_fsevent"))]
    fn remove_event_not_swallowed_after_create() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let dir_path = dir.path().canonicalize()?;

        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(Duration::from_millis(10), None, tx)?;
        debouncer.watch(&dir_path, RecursiveMode::NonRecursive)?;

        // Create a file and wait for the debouncer to deliver the Create event.
        let file_path = dir_path.join("ephemeral.txt");
        fs::write(&file_path, b"will be deleted")?;

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got_create = false;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(events)) => {
                    if events.iter().any(|e| {
                        matches!(e.event.kind, EventKind::Create(_))
                            && e.event.paths.contains(&file_path)
                    }) {
                        got_create = true;
                        break;
                    }
                }
                Ok(Err(_)) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        }
        assert!(got_create, "expected Create event for ephemeral.txt");

        // Drain any remaining events from the creation.
        std::thread::sleep(Duration::from_millis(200));
        while rx.try_recv().is_ok() {}

        // Delete the file.
        fs::remove_file(&file_path)?;

        // The debouncer MUST deliver an event for this path (Remove, or at
        // minimum any event whose path matches so the consumer can stat it).
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got_removal = false;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(events)) => {
                    if events.iter().any(|e| e.event.paths.contains(&file_path)) {
                        got_removal = true;
                        break;
                    }
                }
                Ok(Err(_)) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        }
        assert!(
            got_removal,
            "expected Remove (or any) event for deleted ephemeral.txt, got none within 5s"
        );

        Ok(())
    }

    /// Unit-level reproducer for the same bug: feed a Create + Remove
    /// sequence into `DebounceDataInner` directly, with the queue already
    /// flushed between them (simulating the debounce tick).  The Remove
    /// must not be swallowed.
    #[test]
    #[cfg(target_os = "macos")]
    fn push_remove_after_flushed_create() {
        use crate::NoCache;
        use notify::event::{CreateKind, RemoveKind};
        use std::path::PathBuf;

        let mut state = DebounceDataInner::new(NoCache, Duration::from_millis(50));

        let time = Instant::now();
        MockTime::set_time(time);

        let path = PathBuf::from("/tmp/test_file.txt");

        // Simulate: file created → Create event added
        state.add_event(Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![path.clone()],
            ..Default::default()
        });

        // Simulate: debounce tick flushes the Create
        MockTime::advance(Duration::from_millis(100));
        let flushed = state.debounced_events();
        assert_eq!(flushed.len(), 1);
        assert!(matches!(flushed[0].event.kind, EventKind::Create(_)));

        // Simulate: FSEvents sends Create + Remove for the deletion
        // (this is what macOS does)
        state.add_event(Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![path.clone()],
            ..Default::default()
        });
        state.add_event(Event {
            kind: EventKind::Remove(RemoveKind::File),
            paths: vec![path.clone()],
            ..Default::default()
        });

        // Flush again — we MUST get the Remove event
        MockTime::advance(Duration::from_millis(100));
        let flushed = state.debounced_events();
        assert!(
            flushed
                .iter()
                .any(|e| matches!(e.event.kind, EventKind::Remove(_))),
            "expected Remove event after flushed Create, got: {flushed:?}"
        );
    }

    #[cfg(feature = "futures")]
    #[tokio::test]
    async fn futures_unbounded_sender_as_handler() {
        use futures::StreamExt;

        let dir = tempdir().unwrap();

        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let mut debouncer = new_debouncer(Duration::from_millis(10), None, tx).unwrap();
        debouncer
            .watch(dir.path(), RecursiveMode::Recursive)
            .unwrap();

        let file_path = dir.path().join("file.txt");
        fs::write(&file_path, b"Lorem ipsum").unwrap();

        tokio::time::timeout(Duration::from_secs(10), rx.next())
            .await
            .expect("timeout")
            .expect("No event")
            .expect("error");
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn tokio_unbounded_sender_as_handler() {
        let dir = tempdir().unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut debouncer = new_debouncer(Duration::from_millis(10), None, tx).unwrap();
        debouncer
            .watch(dir.path(), RecursiveMode::Recursive)
            .unwrap();

        let file_path = dir.path().join("file.txt");
        fs::write(&file_path, b"Lorem ipsum").unwrap();

        tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("timeout")
            .expect("No event")
            .expect("error");
    }

    #[test]
    fn update_paths() -> Result<(), Box<dyn std::error::Error>> {
        let dir1 = tempdir()?;
        let dir2 = tempdir()?;

        // set up the watcher
        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(Duration::from_millis(10), None, tx)?;
        debouncer.update_paths([
            PathOp::watch_recursive(dir1.path()),
            PathOp::watch_recursive(dir2.path()),
        ])?;

        // create a new file
        let file_path1 = dir1.path().join("file.txt");
        let file_path2 = dir2.path().join("file.txt");
        fs::write(&file_path1, b"Lorem ipsum1")?;
        fs::write(&file_path2, b"Lorem ipsum1")?;

        println!("waiting for events at {file_path1:?} and {file_path2:?}");

        // wait for up to 10 seconds for the create event, ignore all other events
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut received = (false, false);
        while deadline > Instant::now() {
            let events = rx
                .recv_timeout(deadline - Instant::now())
                .expect("did not receive expected event")
                .expect("received an error");

            for event in events {
                println!("event {event:?}");
                if event.event.paths == vec![file_path1.clone()]
                    || event.event.paths == vec![file_path1.canonicalize()?]
                {
                    received.0 = true;
                }

                if event.event.paths == vec![file_path2.clone()]
                    || event.event.paths == vec![file_path2.canonicalize()?]
                {
                    received.1 = true;
                }

                if received == (true, true) {
                    return Ok(());
                }
            }
        }

        panic!("did not receive expected event");
    }

    /// A handler that reads the changed files must not re-trigger itself.
    #[cfg(target_os = "linux")]
    #[test]
    fn reading_a_file_is_not_reported() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("file.txt");
        fs::write(&file_path, b"Lorem ipsum").unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(Duration::from_millis(50), None, tx).unwrap();
        debouncer
            .watch(dir.path(), RecursiveMode::Recursive)
            .unwrap();

        fs::read(&file_path).unwrap();

        let received = rx.recv_timeout(Duration::from_millis(500));
        assert!(received.is_err(), "unexpected notification: {received:?}");
    }

    #[test]
    fn watcher_construction_error_releases_event_handler() {
        let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();

        let result = new_debouncer_opt::<_, UnconstructibleWatcher, NoCache>(
            test_config(),
            tx,
            NoCache::new(),
        );
        assert!(result.is_err());

        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn tick_rate_above_timeout_is_rejected() {
        let config = test_config().with_tick_rate(Duration::from_millis(21));
        let result = new_debouncer_opt::<_, TrackingWatcher, NoCache>(config, |_| {}, NoCache);
        assert!(result.is_err());
    }

    #[test]
    fn update_paths_error_does_not_add_failed_root() -> Result<(), Box<dyn std::error::Error>> {
        let mut debouncer =
            new_debouncer_opt::<_, FailingWatcher, NoCache>(test_config(), |_| {}, NoCache::new())?;

        let err = debouncer
            .update_paths([
                PathOp::watch_recursive("ok1"),
                PathOp::watch_recursive("bad"),
                PathOp::watch_recursive("ok2"),
            ])
            .unwrap_err();
        assert!(err.origin.is_some());
        assert_eq!(err.remaining.len(), 1);

        let roots: Vec<_> = debouncer
            .data
            .inner
            .lock()
            .unwrap()
            .roots
            .iter()
            .map(|root| (root.path.clone(), root.recursive_mode))
            .collect();
        assert_eq!(
            roots,
            vec![(PathBuf::from("ok1"), RecursiveMode::Recursive)]
        );

        Ok(())
    }

    #[test]
    fn watched_paths_with_watch_update_paths_and_unwatch() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut debouncer = new_debouncer_opt::<_, TrackingWatcher, NoCache>(
            test_config(),
            |_| {},
            NoCache::new(),
        )?;

        let path1 = PathBuf::from("one");
        let path2 = PathBuf::from("two");
        let path3 = PathBuf::from("three");

        assert!(debouncer.watched_paths()?.is_empty());

        debouncer.watch(&path1, RecursiveMode::Recursive)?;
        assert_eq!(
            debouncer.watched_paths()?,
            vec![WatchedPath::new(path1.clone(), RecursiveMode::Recursive)]
        );

        debouncer.update_paths([
            PathOp::unwatch(&path1),
            PathOp::watch_non_recursive(path2.clone()),
            PathOp::watch_recursive(path3.clone()),
        ])?;
        assert_eq!(
            debouncer.watched_paths()?,
            vec![
                WatchedPath::new(path2.clone(), RecursiveMode::NonRecursive),
                WatchedPath::new(path3.clone(), RecursiveMode::Recursive),
            ]
        );

        debouncer.unwatch(&path2)?;
        assert_eq!(
            debouncer.watched_paths()?,
            vec![WatchedPath::new(path3, RecursiveMode::Recursive)]
        );

        Ok(())
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn child_of_a_non_recursive_root_is_cached_without_its_entries()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut debouncer = tracking_debouncer(FileIdMap::new())?;

        let tmpdir = tempdir()?;
        debouncer.watch(tmpdir.path(), RecursiveMode::NonRecursive)?;
        let child = tmpdir.path().join("child");
        let grandchild = child.join("file.txt");
        fs::create_dir(&child)?;
        fs::write(&grandchild, b"content")?;

        // Copy out what the assertions need and release the lock first: a panic while the
        // guard is held poisons the mutex, and the debouncer's drop then panics too.
        let (child_cached, grandchild_cached) = {
            let mut data = debouncer.data.inner.lock().unwrap();
            data.add_event(
                Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                    .add_path(child.clone()),
            );
            (
                data.cache.cached_file_id(&child).is_some(),
                data.cache.cached_file_id(&grandchild).is_some(),
            )
        };
        assert!(child_cached);
        assert!(
            !grandchild_cached,
            "the non-recursive root does not watch the entries below its children"
        );

        Ok(())
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn unwatch_parent_preserves_nested_root() -> Result<(), Box<dyn std::error::Error>> {
        let mut debouncer = new_debouncer_opt::<_, TrackingWatcher, FileIdMap>(
            test_config(),
            |_| {},
            FileIdMap::new(),
        )?;

        let tmpdir = tempdir()?;
        let nested = tmpdir.path().join("nested");
        fs::create_dir(&nested)?;
        let nested_file = nested.join("file.txt");
        fs::write(&nested_file, b"content")?;

        debouncer.watch(tmpdir.path(), RecursiveMode::Recursive)?;
        debouncer.watch(&nested, RecursiveMode::Recursive)?;
        debouncer.unwatch(tmpdir.path())?;

        // Copy out what the assertions need and release the lock first: a panic while the
        // guard is held poisons the mutex, and the debouncer's drop then panics too.
        let (roots, nested_file_cached) = {
            let data = debouncer.data.inner.lock().unwrap();
            let roots = data
                .roots
                .iter()
                .map(|root| root.path.clone())
                .collect::<Vec<_>>();
            let cached = data.cache.cached_file_id(&nested_file).is_some();
            (roots, cached)
        };

        assert_eq!(
            roots,
            vec![nested.clone()],
            "unwatching a parent must not drop roots nested under it"
        );
        assert!(
            nested_file_cached,
            "removing the parent must rebuild cache coverage for the surviving nested root"
        );

        Ok(())
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn unwatch_not_found_preserves_nested_roots() -> Result<(), Box<dyn std::error::Error>> {
        let mut debouncer = new_debouncer_opt::<_, TrackingWatcher, FileIdMap>(
            test_config(),
            |_| {},
            FileIdMap::new(),
        )?;

        let tmpdir = tempdir()?;
        let nested = tmpdir.path().join("nested");
        fs::create_dir(&nested)?;
        let nested_file = nested.join("file.txt");
        fs::write(&nested_file, b"content")?;

        debouncer.watch(&nested, RecursiveMode::Recursive)?;

        // The parent was never watched: the backend reports WatchNotFound and changes
        // nothing, so the error propagates and the nested root's bookkeeping and cached
        // IDs must stay untouched.
        let result = debouncer.unwatch(tmpdir.path());
        assert!(matches!(&result, Err(error) if matches!(error.kind, ErrorKind::WatchNotFound)));

        let (roots, nested_file_cached) = {
            let data = debouncer.data.inner.lock().unwrap();
            let roots = data
                .roots
                .iter()
                .map(|root| root.path.clone())
                .collect::<Vec<_>>();
            let cached = data.cache.cached_file_id(&nested_file).is_some();
            (roots, cached)
        };

        assert_eq!(roots, vec![nested.clone()]);
        assert!(nested_file_cached);

        Ok(())
    }
}
