//! Debouncer for [notify](https://crates.io/crates/notify). Filters incoming events and emits only one event per timeframe per file.
//!
//! # Installation
//!
//! ```toml
//! [dependencies]
//! notify-debouncer-mini = "0.8.0-rc.1"
//! ```
//! In case you want to select specific features of notify,
//! specify notify as dependency explicitly in your dependencies.
//! Otherwise you can just use the re-export of notify from debouncer-mini.
//! ```toml
//! notify-debouncer-mini = "0.8.0-rc.1"
//! notify = { version = "..", features = [".."] }
//! ```
//!
//! # Examples
//! See also the full configuration example [here](https://github.com/notify-rs/notify/blob/main/examples/debouncer_mini_custom.rs).
//!
//! ```rust,no_run
//! # use std::path::Path;
//! # use std::time::Duration;
//! use notify_debouncer_mini::{notify::*, new_debouncer, DebounceEventResult, DebouncedEventBatch};
//!
//! # fn main() {
//!   // Select recommended watcher for debouncer.
//!   // Using a callback here, could also be a channel.
//!   let mut debouncer = new_debouncer(Duration::from_secs(2), |res: DebounceEventResult| {
//!       match res {
//!           Ok(DebouncedEventBatch::Events(events)) => {
//!               for event in events {
//!                   println!("Event {:?} for {:?}", event.kind, event.path);
//!               }
//!           }
//!           Ok(DebouncedEventBatch::Rescan) => {
//!               // Refresh any cached state for all watched paths here.
//!               println!("Events were missed; rescan all watched paths");
//!           }
//!           Err(e) => println!("Error {:?}",e),
//!       }
//!   }).unwrap();
//!
//!   // Add a path to be watched. All files and directories at that path and
//!   // below will be monitored for changes.
//!   debouncer.watcher().watch(Path::new("."), RecursiveMode::Recursive).unwrap();
//!
//!   // note that dropping the debouncer (as will happen here) also ends the debouncer
//!   // thus this demo would need an endless loop to keep running
//! # }
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
//! - `crossbeam-channel`, `flume`, `futures`, and `tokio` enable the corresponding
//!   channel senders as event handlers
//! - `serialization-compat-6` restores notify 6 serialization behavior
//!
//! # Caveats
//!
//! As all file events are sourced from notify, the [known problems](https://docs.rs/notify/latest/notify/#known-problems) section applies here too.
//!
//! If the backend reports missed events, [`DebouncedEventBatch::Rescan`] is delivered
//! without waiting for the debounce timeout. It applies to all watched paths, even if
//! the original event included paths. Handle it before filtering individual path changes.
//! Pending path changes retain their usual debounce deadlines.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{RecvTimeoutError, Sender},
    time::{Duration, Instant},
};

pub use notify;
pub use notify_types::debouncer_mini::{DebouncedEvent, DebouncedEventBatch, DebouncedEventKind};

use notify::{Error, Event, RecommendedWatcher, Watcher};

/// The set of requirements for watcher debounce event handling functions.
///
/// # Example implementation
///
/// ```rust,no_run
/// # use notify::{Event, Result, EventHandler};
/// # use notify_debouncer_mini::{DebounceEventHandler, DebounceEventResult, DebouncedEventBatch};
///
/// /// Prints received events
/// struct EventPrinter;
///
/// impl DebounceEventHandler for EventPrinter {
///     fn handle_event(&mut self, event: DebounceEventResult) {
///         match event {
///             Ok(DebouncedEventBatch::Events(events)) => {
///                 for event in events {
///                     println!("Event {:?} for path {:?}",event.kind,event.path);
///                 }
///             },
///             Ok(DebouncedEventBatch::Rescan) => {
///                 // Refresh any cached state for all watched paths here.
///                 println!("Events were missed; rescan all watched paths");
///             },
///             // errors are immediately reported
///             Err(error) => println!("Got error {:?}",error),
///         }
///     }
/// }
/// ```
pub trait DebounceEventHandler: Send + 'static {
    /// Handles an event.
    fn handle_event(&mut self, event: DebounceEventResult);
}

/// Config for debouncer-mini
/// ```rust
/// # use std::time::Duration;
/// use notify_debouncer_mini::Config;
/// let backend_config = notify::Config::default();
///
/// let config = Config::default().with_timeout(Duration::from_secs(1)).with_batch_mode(true)
///     .with_notify_config(backend_config);
/// ```
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Config {
    timeout: Duration,
    batch_mode: bool,
    notify_config: notify::Config,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(500),
            batch_mode: true,
            notify_config: notify::Config::default(),
        }
    }
}

impl Config {
    /// Set timeout
    ///
    /// Timeout is the amount of time after which a debounced event is emitted or a continuous event is send, if there still are events incoming for the specific path.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    /// Set batch mode
    ///
    /// When `batch_mode` is enabled, events may be delayed (at most 2x the specified timeout) and delivered with others.
    /// If disabled, all events are delivered immediately when their debounce timeout is reached.
    #[must_use]
    pub fn with_batch_mode(mut self, batch_mode: bool) -> Self {
        self.batch_mode = batch_mode;
        self
    }
    /// Set [`notify::Config`] for the backend
    #[must_use]
    pub fn with_notify_config(mut self, notify_config: notify::Config) -> Self {
        self.notify_config = notify_config;
        self
    }
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

/// Deduplicate event data entry
#[derive(Debug)]
struct EventData {
    /// Insertion Time
    insert: Instant,
    /// Last Update
    update: Instant,
}

impl EventData {
    #[inline(always)]
    fn new_any(time: Instant) -> Self {
        Self {
            insert: time,
            update: time,
        }
    }
}

/// A result of debounced events.
///
/// Contains a batch of path changes, an immediate rescan notification, or an immediate error.
pub type DebounceEventResult = Result<DebouncedEventBatch, Error>;

enum InnerEvent {
    NotifyEvent(Result<Event, Error>),
    Shutdown,
}

struct DebounceDataInner {
    /// Path -> Event data
    event_map: HashMap<PathBuf, EventData>,
    /// timeout used to compare all events against, config
    timeout: Duration,
    /// Whether to time events exactly, or batch multiple together.
    /// This reduces the amount of updates but possibly waiting longer than necessary for some events
    batch_mode: bool,
    /// next debounce deadline
    debounce_deadline: Option<Instant>,
}

impl DebounceDataInner {
    pub fn new(timeout: Duration, batch_mode: bool) -> Self {
        Self {
            timeout,
            debounce_deadline: None,
            event_map: Default::default(),
            batch_mode,
        }
    }

    /// Returns a duration to wait for the next tick
    #[inline]
    pub fn next_tick(&self) -> Option<Duration> {
        self.debounce_deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    /// Retrieve a vec of debounced events, removing them if not continuous
    ///
    /// Updates the internal tracker for the next tick
    pub fn debounced_events(&mut self) -> Vec<DebouncedEvent> {
        let mut events_expired = Vec::with_capacity(self.event_map.len());
        let mut data_back = HashMap::with_capacity(self.event_map.len());
        // TODO: perfect fit for drain_filter https://github.com/rust-lang/rust/issues/59618
        // reset deadline
        self.debounce_deadline = None;
        for (path, event) in self.event_map.drain() {
            if event.update.elapsed() >= self.timeout {
                log::trace!("debounced event: {:?}", DebouncedEventKind::Any);
                events_expired.push(DebouncedEvent::new(path, DebouncedEventKind::Any));
            } else if event.insert.elapsed() >= self.timeout {
                log::trace!("debounced event: {:?}", DebouncedEventKind::AnyContinuous);
                // set a new deadline, otherwise an 'AnyContinuous' will never resolve to a final 'Any' event
                Self::check_deadline(
                    self.batch_mode,
                    self.timeout,
                    &mut self.debounce_deadline,
                    &event,
                );
                data_back.insert(path.clone(), event);
                events_expired.push(DebouncedEvent::new(path, DebouncedEventKind::AnyContinuous));
            } else {
                // event is neither old enough for continuous event, nor is it expired for an Any event
                Self::check_deadline(
                    self.batch_mode,
                    self.timeout,
                    &mut self.debounce_deadline,
                    &event,
                );
                data_back.insert(path, event);
            }
        }
        self.event_map = data_back;
        events_expired
    }

    /// Updates the deadline if none is set or when batch mode is disabled and the current deadline would miss the next event.
    /// The new deadline is calculated based on the last event update time and the debounce timeout.
    ///
    /// can't sub-function this due to `event_map.drain()` holding `&mut self`
    fn check_deadline(
        batch_mode: bool,
        timeout: Duration,
        debounce_deadline: &mut Option<Instant>,
        event: &EventData,
    ) {
        let deadline_candidate = event.update + timeout;
        match debounce_deadline {
            Some(current_deadline) => {
                // shorten deadline to not delay the event
                // with batch mode simply wait for the incoming deadline and delay the event until then
                if !batch_mode && *current_deadline > deadline_candidate {
                    *debounce_deadline = Some(deadline_candidate);
                }
            }
            None => *debounce_deadline = Some(deadline_candidate),
        }
    }

    /// Add new event to debouncer cache
    #[inline(always)]
    fn add_event(&mut self, event: Event) {
        log::trace!("raw event: {event:?}");
        let time = Instant::now();
        if self.debounce_deadline.is_none() {
            self.debounce_deadline = Some(time + self.timeout);
        }
        for path in event.paths.into_iter() {
            if let Some(v) = self.event_map.get_mut(&path) {
                v.update = time;
            } else {
                self.event_map.insert(path, EventData::new_any(time));
            }
        }
    }
}

/// Debouncer guard, stops the debouncer on drop
#[derive(Debug)]
pub struct Debouncer<T: Watcher> {
    watcher: T,
    stop_channel: Sender<InnerEvent>,
}

impl<T: Watcher> Debouncer<T> {
    /// Access to the internally used notify Watcher backend
    pub fn watcher(&mut self) -> &mut dyn Watcher {
        &mut self.watcher
    }
}

impl<T: Watcher> Drop for Debouncer<T> {
    fn drop(&mut self) {
        // send error just means that it is stopped, can't do much else
        let _ = self.stop_channel.send(InnerEvent::Shutdown);
    }
}

/// Creates a new debounced watcher with custom configuration.
pub fn new_debouncer_opt<F: DebounceEventHandler, T: Watcher>(
    config: Config,
    mut event_handler: F,
) -> Result<Debouncer<T>, Error> {
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::Builder::new()
        .name("notify-rs debouncer loop".to_string())
        .spawn(move || {
            let mut data = DebounceDataInner::new(config.timeout, config.batch_mode);
            loop {
                let event = match data.next_tick() {
                    Some(timeout) => rx.recv_timeout(timeout),
                    None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };

                match event {
                    Ok(InnerEvent::NotifyEvent(Ok(event))) if event.need_rescan() => {
                        event_handler.handle_event(Ok(DebouncedEventBatch::Rescan));
                    }
                    Ok(InnerEvent::NotifyEvent(Ok(event))) => data.add_event(event),
                    Ok(InnerEvent::NotifyEvent(Err(err))) => event_handler.handle_event(Err(err)),
                    Err(RecvTimeoutError::Timeout) => {
                        let events = data.debounced_events();
                        if !events.is_empty() {
                            event_handler.handle_event(Ok(DebouncedEventBatch::Events(events)));
                        }
                    }
                    Ok(InnerEvent::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        })?;

    let tx_c = tx.clone();
    let watcher = T::new(
        move |e: Result<Event, Error>| {
            // send failure can't be handled, would need a working channel to signal that
            // also probably means that we're in the process of shutting down
            let _ = tx_c.send(InnerEvent::NotifyEvent(e));
        },
        config.notify_config,
    )?;

    let guard = Debouncer {
        watcher,
        stop_channel: tx,
    };

    Ok(guard)
}

/// Short function to create a new debounced watcher with the recommended debouncer.
///
/// Timeout is the amount of time after which a debounced event is emitted or a continuous event is send, if there still are events incoming for the specific path.
pub fn new_debouncer<F: DebounceEventHandler>(
    timeout: Duration,
    event_handler: F,
) -> Result<Debouncer<RecommendedWatcher>, Error> {
    let config = Config::default().with_timeout(timeout);
    new_debouncer_opt::<F, RecommendedWatcher>(config, event_handler)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{EventKind, RecursiveMode, event::Flag};
    use std::{fs, path::Path};
    use tempfile::tempdir;

    struct TestWatcher {
        event_handler: Box<dyn notify::EventHandler>,
    }

    impl Watcher for TestWatcher {
        fn new<F: notify::EventHandler>(
            event_handler: F,
            _config: notify::Config,
        ) -> notify::Result<Self> {
            Ok(Self {
                event_handler: Box::new(event_handler),
            })
        }

        fn watch(&mut self, _path: &Path, _recursive_mode: RecursiveMode) -> notify::Result<()> {
            Ok(())
        }

        fn unwatch(&mut self, _path: &Path) -> notify::Result<()> {
            Ok(())
        }

        fn kind() -> notify::WatcherKind {
            notify::WatcherKind::NullWatcher
        }
    }

    #[test]
    fn rescans_are_delivered_without_debouncing() {
        for batch_mode in [false, true] {
            for has_pending_event in [false, true] {
                let (tx, rx) = std::sync::mpsc::channel();
                let config = Config::default()
                    .with_timeout(Duration::from_secs(60))
                    .with_batch_mode(batch_mode);
                let mut debouncer = new_debouncer_opt::<_, TestWatcher>(config, tx).unwrap();
                let handler = &mut debouncer.watcher.event_handler;

                if has_pending_event {
                    handler.handle_event(Ok(Event::new(EventKind::Any).add_path("pending".into())));
                }

                // Rescan takes precedence over both the kind and any paths, including no paths.
                for event in [
                    Event::new(EventKind::Other),
                    Event::new(EventKind::Any).add_path("watched".into()),
                    Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
                        .add_path("first".into())
                        .add_path("second".into()),
                ] {
                    handler.handle_event(Ok(event.set_flag(Flag::Rescan)));
                    assert_eq!(
                        rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
                        DebouncedEventBatch::Rescan,
                    );
                }
            }
        }
    }

    #[test]
    fn rescan_preserves_pending_and_subsequent_path_changes() {
        for batch_mode in [false, true] {
            let (tx, rx) = std::sync::mpsc::channel();
            let config = Config::default()
                .with_timeout(Duration::from_millis(50))
                .with_batch_mode(batch_mode);
            let mut debouncer = new_debouncer_opt::<_, TestWatcher>(config, tx).unwrap();
            let handler = &mut debouncer.watcher.event_handler;

            handler.handle_event(Ok(Event::new(EventKind::Any).add_path("before".into())));
            handler.handle_event(Ok(Event::new(EventKind::Other)
                .set_flag(Flag::Rescan)
                .add_path("rescan-scope".into())));
            handler.handle_event(Ok(Event::new(EventKind::Any).add_path("after".into())));

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut changes = Vec::new();
            let mut received_rescan = false;
            while changes.len() < 2 || !received_rescan {
                match rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .unwrap()
                    .unwrap()
                {
                    DebouncedEventBatch::Events(events) => {
                        assert!(!events.is_empty());
                        changes.extend(events);
                    }
                    DebouncedEventBatch::Rescan => {
                        assert!(!received_rescan, "duplicate rescan notification");
                        received_rescan = true;
                    }
                }
            }

            changes.sort_by(|a, b| a.path.cmp(&b.path));
            assert_eq!(
                changes,
                vec![
                    DebouncedEvent::new("after".into(), DebouncedEventKind::Any),
                    DebouncedEvent::new("before".into(), DebouncedEventKind::Any),
                ],
            );
        }
    }

    #[test]
    fn errors_are_delivered_without_debouncing() {
        for has_pending_event in [false, true] {
            let (tx, rx) = std::sync::mpsc::channel();
            let config = Config::default().with_timeout(Duration::from_secs(60));
            let mut debouncer = new_debouncer_opt::<_, TestWatcher>(config, tx).unwrap();
            let handler = &mut debouncer.watcher.event_handler;

            if has_pending_event {
                handler.handle_event(Ok(Event::new(EventKind::Any).add_path("pending".into())));
            }
            handler.handle_event(Err(Error::path_not_found().add_path("missing".into())));

            let error = rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap_err();
            assert!(matches!(error.kind, notify::ErrorKind::PathNotFound));
            assert_eq!(error.paths, vec![PathBuf::from("missing")]);
        }
    }

    #[test]
    fn integration() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;

        // set up the watcher
        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(Duration::from_secs(1), tx)?;
        debouncer
            .watcher()
            .watch(dir.path(), RecursiveMode::Recursive)?;

        // create a new file
        let file_path = dir.path().join("file.txt");
        fs::write(&file_path, b"Lorem ipsum")?;

        println!("waiting for event at {}", file_path.display());

        // wait for up to 10 seconds for the create event, ignore all other events
        let deadline = Instant::now() + Duration::from_secs(10);
        while deadline > Instant::now() {
            let batch = rx
                .recv_timeout(deadline - Instant::now())
                .expect("did not receive expected event")
                .expect("received an error");
            let DebouncedEventBatch::Events(events) = batch else {
                panic!("unexpected rescan notification");
            };

            for event in events {
                if event == DebouncedEvent::new(file_path.clone(), DebouncedEventKind::Any)
                    || event
                        == DebouncedEvent::new(file_path.canonicalize()?, DebouncedEventKind::Any)
                {
                    return Ok(());
                }

                println!("unexpected event: {event:?}");
            }
        }

        panic!("did not receive expected event");
    }

    #[cfg(feature = "futures")]
    #[tokio::test]
    async fn futures_unbounded_sender_as_handler() {
        use futures::StreamExt;

        let dir = tempdir().unwrap();

        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let mut debouncer = new_debouncer(Duration::from_secs(1), tx).unwrap();
        debouncer
            .watcher
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
        let mut debouncer = new_debouncer(Duration::from_secs(1), tx).unwrap();
        debouncer
            .watcher
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
}
