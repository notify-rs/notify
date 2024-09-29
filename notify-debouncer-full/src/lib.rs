//! A debouncer for [notify] that is optimized for ease of use.
//!
//! * Only emits a single `Rename` event if the rename `From` and `To` events can be matched
//! * Merges multiple `Rename` events
//! * Takes `Rename` events into account and updates paths for events that occurred before the rename event, but which haven't been emitted, yet
//! * Optionally keeps track of the file system IDs all files and stiches rename events together (FSevents, Windows)
//! * Emits only one `Remove` event when deleting a directory (inotify)
//! * Doesn't emit duplicate create events
//! * Doesn't emit `Modify` events after a `Create` event
//!
//! # Installation
//!
//! ```toml
//! [dependencies]
//! notify-debouncer-full = "0.3.1"
//! ```
//!
//! In case you want to select specific features of notify,
//! specify notify as dependency explicitly in your dependencies.
//! Otherwise you can just use the re-export of notify from debouncer-full.
//!
//! ```toml
//! notify-debouncer-full = "0.3.1"
//! notify = { version = "..", features = [".."] }
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
//! debouncer.watcher().watch(Path::new("."), RecursiveMode::Recursive).unwrap();
//!
//! // Add the same path to the file ID cache. The cache uses unique file IDs
//! // provided by the file system and is used to stich together rename events
//! // in case the notification back-end doesn't emit rename cookies.
//! debouncer.cache().add_root(Path::new("."), RecursiveMode::Recursive);
//! ```
//!
//! # Features
//!
//! The following crate features can be turned on or off in your cargo dependency config:
//!
//! - `crossbeam` enabled by default, adds [`DebounceEventHandler`](DebounceEventHandler) support for crossbeam channels.
//!   Also enables crossbeam-channel in the re-exported notify. You may want to disable this when using the tokio async runtime.
//! - `serde` enables serde support for events.
//!
//! # Caveats
//!
//! As all file events are sourced from notify, the [known problems](https://docs.rs/notify/latest/notify/#known-problems) section applies here too.

mod cache;
mod debounced_event;

#[cfg(test)]
mod testing;

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub use cache::{FileIdCache, FileIdMap, NoCache};
pub use debounced_event::DebouncedEvent;

pub use file_id;
pub use notify;

use file_id::FileId;
use notify::{
    event::{ModifyKind, RemoveKind, RenameMode},
    Error, ErrorKind, Event, EventKind, RecommendedWatcher, Watcher,
};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};

#[cfg(test)]
use mock_instant::Instant;

#[cfg(not(test))]
use std::time::Instant;

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

#[cfg(feature = "crossbeam")]
impl DebounceEventHandler for crossbeam_channel::Sender<DebounceEventResult> {
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

type DebounceData<T> = Arc<Mutex<DebounceDataInner<T>>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Queue {
    /// Events must be stored in the following order:
    /// 1. `remove` or `move out` event
    /// 2. `rename` event
    /// 3. Other events
    events: VecDeque<DebouncedEvent>,
}

impl Queue {
    fn was_created(&self) -> bool {
        self.events.front().map_or(false, |event| {
            matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To))
            )
        })
    }

    fn was_removed(&self) -> bool {
        self.events.front().map_or(false, |event| {
            matches!(
                event.kind,
                EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From))
            )
        })
    }
}

#[derive(Debug)]
pub(crate) struct DebounceDataInner<T> {
    queues: HashMap<PathBuf, Queue>,
    cache: T,
    rename_event: Option<(DebouncedEvent, Option<FileId>)>,
    rescan_event: Option<DebouncedEvent>,
    errors: Vec<Error>,
    timeout: Duration,
}

impl<T: FileIdCache> DebounceDataInner<T> {
    pub(crate) fn new(cache: T, timeout: Duration) -> Self {
        Self {
            queues: HashMap::new(),
            cache,
            rename_event: None,
            rescan_event: None,
            errors: Vec::new(),
            timeout,
        }
    }

    /// Retrieve a vec of debounced events, removing them if not continuous
    pub fn debounced_events(&mut self) -> Vec<DebouncedEvent> {
        let now = Instant::now();
        let mut events_expired = Vec::with_capacity(self.queues.len());
        let mut queues_remaining = HashMap::with_capacity(self.queues.len());

        if let Some(event) = self.rescan_event.take() {
            if now.saturating_duration_since(event.time) >= self.timeout {
                log::trace!("debounced event: {event:?}");
                events_expired.push(event);
            } else {
                self.rescan_event = Some(event);
            }
        }

        // TODO: perfect fit for drain_filter https://github.com/rust-lang/rust/issues/59618
        for (path, mut queue) in self.queues.drain() {
            let mut kind_index = HashMap::new();

            while let Some(event) = queue.events.pop_front() {
                if now.saturating_duration_since(event.time) >= self.timeout {
                    // remove previous event of the same kind
                    if let Some(idx) = kind_index.get(&event.kind).copied() {
                        events_expired.remove(idx);

                        kind_index.values_mut().for_each(|i| {
                            if *i > idx {
                                *i -= 1
                            }
                        })
                    }

                    kind_index.insert(event.kind, events_expired.len());

                    events_expired.push(event);
                } else {
                    queue.events.push_front(event);
                    break;
                }
            }

            if !queue.events.is_empty() {
                queues_remaining.insert(path, queue);
            }
        }

        self.queues = queues_remaining;

        sort_events(events_expired)
    }

    /// Returns all currently stored errors
    pub fn errors(&mut self) -> Vec<Error> {
        let mut v = Vec::new();
        std::mem::swap(&mut v, &mut self.errors);
        v
    }

    /// Add an error entry to re-send later on
    pub fn add_error(&mut self, error: Error) {
        self.errors.push(error);
    }

    /// Add new event to debouncer cache
    pub fn add_event(&mut self, event: Event) {
        log::trace!("raw event: {event:?}");

        if event.need_rescan() {
            self.cache.rescan();
            self.rescan_event = Some(event.into());
            return;
        }

        let path = &event.paths[0];

        match &event.kind {
            EventKind::Create(_) => {
                self.cache.add_path(path);

                self.push_event(event, Instant::now());
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
                self.push_remove_event(event, Instant::now());
            }
            EventKind::Other => {
                // ignore meta events
            }
            _ => {
                if self.cache.cached_file_id(path).is_none() {
                    self.cache.add_path(path);
                }

                self.push_event(event, Instant::now());
            }
        }
    }

    fn handle_rename_from(&mut self, event: Event) {
        let time = Instant::now();
        let path = &event.paths[0];

        // store event
        let file_id = self.cache.cached_file_id(path).cloned();
        self.rename_event = Some((DebouncedEvent::new(event.clone(), time), file_id));

        self.cache.remove_path(path);

        self.push_event(event, time);
    }

    fn handle_rename_to(&mut self, event: Event) {
        self.cache.add_path(&event.paths[0]);

        let trackers_match = self
            .rename_event
            .as_ref()
            .and_then(|(e, _)| e.tracker())
            .and_then(|from_tracker| {
                event
                    .attrs
                    .tracker()
                    .map(|to_tracker| from_tracker == to_tracker)
            })
            .unwrap_or_default();

        let file_ids_match = self
            .rename_event
            .as_ref()
            .and_then(|(_, id)| id.as_ref())
            .and_then(|from_file_id| {
                self.cache
                    .cached_file_id(&event.paths[0])
                    .map(|to_file_id| from_file_id == to_file_id)
            })
            .unwrap_or_default();

        if trackers_match || file_ids_match {
            // connect rename
            let (mut rename_event, _) = self.rename_event.take().unwrap(); // unwrap is safe because `rename_event` must be set at this point
            let path = rename_event.paths.remove(0);
            let time = rename_event.time;
            self.push_rename_event(path, event, time);
        } else {
            // move in
            self.push_event(event, Instant::now());
        }

        self.rename_event = None;
    }

    fn push_rename_event(&mut self, path: PathBuf, event: Event, time: Instant) {
        self.cache.remove_path(&path);

        let mut source_queue = self.queues.remove(&path).unwrap_or_default();

        // remove rename `from` event
        source_queue.events.pop_back();

        // remove existing rename event
        let (remove_index, original_path, original_time) = source_queue
            .events
            .iter()
            .enumerate()
            .find_map(|(index, e)| {
                if matches!(
                    e.kind,
                    EventKind::Modify(ModifyKind::Name(RenameMode::Both))
                ) {
                    Some((Some(index), e.paths[0].clone(), e.time))
                } else {
                    None
                }
            })
            .unwrap_or((None, path, time));

        if let Some(remove_index) = remove_index {
            source_queue.events.remove(remove_index);
        }

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
            source_queue.events.push_front(DebouncedEvent {
                event: Event {
                    kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                    paths: vec![original_path, event.paths[0].clone()],
                    attrs: event.attrs,
                },
                time: original_time,
            });
        }

        if let Some(target_queue) = self.queues.get_mut(&event.paths[0]) {
            if !target_queue.was_created() {
                let mut remove_event = DebouncedEvent {
                    event: Event {
                        kind: EventKind::Remove(RemoveKind::Any),
                        paths: vec![event.paths[0].clone()],
                        attrs: Default::default(),
                    },
                    time: original_time,
                };
                if !target_queue.was_removed() {
                    remove_event.event = remove_event.event.set_info("override");
                }
                source_queue.events.push_front(remove_event);
            }
            *target_queue = source_queue;
        } else {
            self.queues.insert(event.paths[0].clone(), source_queue);
        }
    }

    fn push_remove_event(&mut self, event: Event, time: Instant) {
        let path = &event.paths[0];

        // remove child queues
        self.queues.retain(|p, _| !p.starts_with(path) || p == path);

        // remove cached file ids
        self.cache.remove_path(path);

        match self.queues.get_mut(path) {
            Some(queue) if queue.was_created() => {
                self.queues.remove(path);
            }
            Some(queue) => {
                queue.events = [DebouncedEvent::new(event, time)].into();
            }
            None => {
                self.push_event(event, time);
            }
        }
    }

    fn push_event(&mut self, event: Event, time: Instant) {
        let path = &event.paths[0];

        if let Some(queue) = self.queues.get_mut(path) {
            // skip duplicate create events and modifications right after creation
            if match event.kind {
                EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))
                | EventKind::Create(_) => !queue.was_created(),
                _ => true,
            } {
                queue.events.push_back(DebouncedEvent::new(event, time));
            }
        } else {
            self.queues.insert(
                path.to_path_buf(),
                Queue {
                    events: [DebouncedEvent::new(event, time)].into(),
                },
            );
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
    /// Stop the debouncer, waits for the event thread to finish.
    /// May block for the duration of one tick_rate.
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
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Access to the internally used notify Watcher backend
    pub fn watcher(&mut self) -> &mut T {
        &mut self.watcher
    }

    /// Access to the internally used notify Watcher backend
    pub fn cache(&mut self) -> MappedMutexGuard<C> {
        MutexGuard::map(self.data.lock(), |data| &mut data.cache)
    }
}

impl<T: Watcher, C: FileIdCache> Drop for Debouncer<T, C> {
    fn drop(&mut self) {
        self.set_stop();
    }
}

/// Creates a new debounced watcher with custom configuration.
///
/// Timeout is the amount of time after which a debounced event is emitted.
///
/// If tick_rate is None, notify will select a tick rate that is 1/4 of the provided timeout.
pub fn new_debouncer_opt<F: DebounceEventHandler, T: Watcher, C: FileIdCache + Send + 'static>(
    timeout: Duration,
    tick_rate: Option<Duration>,
    mut event_handler: F,
    file_id_cache: C,
    config: notify::Config,
) -> Result<Debouncer<T, C>, Error> {
    let data = Arc::new(Mutex::new(DebounceDataInner::new(file_id_cache, timeout)));
    let stop = Arc::new(AtomicBool::new(false));

    let tick_div = 4;
    let tick = match tick_rate {
        Some(v) => {
            if v > timeout {
                return Err(Error::new(ErrorKind::Generic(format!(
                    "Invalid tick_rate, tick rate {:?} > {:?} timeout!",
                    v, timeout
                ))));
            }
            v
        }
        None => timeout.checked_div(tick_div).ok_or_else(|| {
            Error::new(ErrorKind::Generic(format!(
                "Failed to calculate tick as {:?}/{}!",
                timeout, tick_div
            )))
        })?,
    };

    let data_c = data.clone();
    let stop_c = stop.clone();
    let thread = std::thread::Builder::new()
        .name("notify-rs debouncer loop".to_string())
        .spawn(move || loop {
            if stop_c.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(tick);
            let send_data;
            let errors;
            {
                let mut lock = data_c.lock();
                send_data = lock.debounced_events();
                errors = lock.errors();
            }
            if !send_data.is_empty() {
                event_handler.handle_event(Ok(send_data));
            }
            if !errors.is_empty() {
                event_handler.handle_event(Err(errors));
            }
        })?;

    let data_c = data.clone();
    let watcher = T::new(
        move |e: Result<Event, Error>| {
            let mut lock = data_c.lock();

            match e {
                Ok(e) => lock.add_event(e),
                // can't have multiple TX, so we need to pipe that through our debouncer
                Err(e) => lock.add_error(e),
            }
        },
        config,
    )?;

    let guard = Debouncer {
        watcher,
        debouncer_thread: Some(thread),
        data,
        stop,
    };

    Ok(guard)
}

/// Short function to create a new debounced watcher with the recommended debouncer and the built-in file ID cache.
///
/// Timeout is the amount of time after which a debounced event is emitted.
///
/// If tick_rate is None, notify will select a tick rate that is 1/4 of the provided timeout.
pub fn new_debouncer<F: DebounceEventHandler>(
    timeout: Duration,
    tick_rate: Option<Duration>,
    event_handler: F,
) -> Result<Debouncer<RecommendedWatcher, FileIdMap>, Error> {
    new_debouncer_opt::<F, RecommendedWatcher, FileIdMap>(
        timeout,
        tick_rate,
        event_handler,
        FileIdMap::new(),
        notify::Config::default(),
    )
}

fn sort_events(events: Vec<DebouncedEvent>) -> Vec<DebouncedEvent> {
    let mut sorted = Vec::with_capacity(events.len());

    // group events by path
    let mut events_by_path: HashMap<_, VecDeque<_>> =
        events.into_iter().fold(HashMap::new(), |mut acc, event| {
            acc.entry(event.paths.last().cloned().unwrap_or_default())
                .or_default()
                .push_back(event);
            acc
        });

    // push events for different paths in chronological order and keep the order of events with the same path

    let mut min_time_heap = events_by_path
        .iter()
        .map(|(path, events)| Reverse((events[0].time, path.clone())))
        .collect::<BinaryHeap<_>>();

    while let Some(Reverse((min_time, path))) = min_time_heap.pop() {
        // unwrap is safe because only paths from `events_by_path` are added to `min_time_heap`
        // and they are never removed from `events_by_path`.
        let events = events_by_path.get_mut(&path).unwrap();

        let mut push_next = false;

        while events.front().map_or(false, |event| event.time <= min_time) {
            // unwrap is safe beause `pop_front` mus return some in order to enter the loop
            let event = events.pop_front().unwrap();
            sorted.push(event);
            push_next = true;
        }

        if push_next {
            if let Some(event) = events.front() {
                min_time_heap.push(Reverse((event.time, path)));
            }
        }
    }

    sorted
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;

    use mock_instant::MockClock;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use testing::TestCase;

    #[rstest]
    fn state(
        #[values(
            "add_create_event",
            "add_create_event_after_remove_event",
            "add_create_dir_event_twice",
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
            "add_rename_both_event",
            "add_remove_event",
            "add_remove_event_after_create_event",
            "add_remove_event_after_modify_event",
            "add_remove_event_after_create_and_modify_event",
            "add_remove_parent_event_after_remove_child_event",
            "add_errors",
            "emit_continuous_modify_content_events",
            "emit_events_in_chronological_order",
            "emit_events_with_a_prepended_rename_event",
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

        MockClock::set_time(Duration::default());

        let time = Instant::now();

        let mut state = test_case.state.into_debounce_data_inner(time);

        for event in test_case.events {
            let event = event.into_debounced_event(time, None);
            MockClock::set_time(event.time - time);
            state.add_event(event.event);
        }

        for error in test_case.errors {
            let e = error.into_notify_error();
            state.add_error(e);
        }

        let expected_errors = std::mem::take(&mut test_case.expected.errors);
        let expected_events = std::mem::take(&mut test_case.expected.events);
        let expected_state = test_case.expected.into_debounce_data_inner(time);
        assert_eq!(
            state.queues, expected_state.queues,
            "queues not as expected"
        );
        assert_eq!(
            state.rename_event, expected_state.rename_event,
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
                .map(|e| format!("{:?}", e))
                .collect::<Vec<_>>(),
            expected_errors
                .iter()
                .map(|e| format!("{:?}", e.clone().into_notify_error()))
                .collect::<Vec<_>>(),
            "errors not as expected"
        );

        let backup_time = Instant::now().duration_since(time);
        let backup_queues = state.queues.clone();

        for (delay, events) in expected_events {
            MockClock::set_time(backup_time);
            state.queues = backup_queues.clone();

            match delay.as_str() {
                "none" => {}
                "short" => MockClock::advance(Duration::from_millis(10)),
                "long" => MockClock::advance(Duration::from_millis(100)),
                _ => {
                    if let Ok(ts) = delay.parse::<u64>() {
                        let ts = time + Duration::from_millis(ts);
                        MockClock::set_time(ts - time);
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
        }
    }
}
