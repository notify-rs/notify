//! Debouncer for [notify](https://crates.io/crates/notify). Filters incoming events and emits only one event per timeframe per file.
//!
//! # Installation
//!
//! ```toml
//! [dependencies]
//! notify-debouncer-mini = "0.6.0"
//! ```
//! In case you want to select specific features of notify,
//! specify notify as dependency explicitly in your dependencies.
//! Otherwise you can just use the re-export of notify from debouncer-mini.
//! ```toml
//! notify-debouncer-mini = "0.6.0"
//! notify = { version = "..", features = [".."] }
//! ```
//!
//! # Examples
//! See also the full configuration example [here](https://github.com/notify-rs/notify/blob/main/examples/debouncer_mini_custom.rs).
//!
//! ```rust,no_run
//! # use std::path::Path;
//! # use std::time::Duration;
//! use notify_debouncer_mini::{notify::*,new_debouncer,DebounceEventResult};
//!
//! # fn main() {
//!   // Select recommended watcher for debouncer.
//!   // Using a callback here, could also be a channel.
//!   let mut debouncer = new_debouncer(Duration::from_secs(2), |res: DebounceEventResult| {
//!       match res {
//!           Ok(events) => events.iter().for_each(|e|println!("Event {:?} for {:?}",e.kind,e.path)),
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
//! The following crate features can be turned on or off in your cargo dependency config:
//!
//! - `serde` passed down to notify-types, off by default
//! - `crossbeam-channel` passed down to notify, off by default
//! - `flume` passed down to notify, off by default
//! - `macos_fsevent` passed down to notify, off by default
//! - `macos_kqueue` passed down to notify, off by default
//! - `serialization-compat-6` passed down to notify, off by default
//!
//! # Caveats
//!
//! As all file events are sourced from notify, the [known problems](https://docs.rs/notify/latest/notify/#known-problems) section applies here too.
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};

pub use notify;
pub use notify_types::debouncer_mini::{DebouncedEvent, DebouncedEventKind};

use notify::{Error, Event, RecommendedWatcher, Watcher};

/// The set of requirements for watcher debounce event handling functions.
///
/// # Example implementation
///
/// ```rust,no_run
/// # use notify::{Event, Result, EventHandler};
/// # use notify_debouncer_mini::{DebounceEventHandler,DebounceEventResult};
///
/// /// Prints received events
/// struct EventPrinter;
///
/// impl DebounceEventHandler for EventPrinter {
///     fn handle_event(&mut self, event: DebounceEventResult) {
///         match event {
///             Ok(events) => {
///                 for event in events {
///                     println!("Event {:?} for path {:?}",event.kind,event.path);
///                 }
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

macro_rules! impl_channel_debounce_handler {
    ($channel:ty, $send:ident) => {
        impl DebounceEventHandler for $channel {
            fn handle_event(&mut self, event: DebounceEventResult) {
                let _ = self.$send(event);
            }
        }
    };
}

#[cfg(feature = "crossbeam-channel")]
impl_channel_debounce_handler!(crossbeam_channel::Sender<DebounceEventResult>, send);

#[cfg(feature = "flume")]
impl_channel_debounce_handler!(flume::Sender<DebounceEventResult>, send);

#[cfg(feature = "futures")]
impl_channel_debounce_handler!(
    futures::channel::mpsc::UnboundedSender<DebounceEventResult>,
    unbounded_send
);

#[cfg(feature = "tokio")]
impl_channel_debounce_handler!(
    tokio::sync::mpsc::UnboundedSender<DebounceEventResult>,
    send
);

// std
impl_channel_debounce_handler!(std::sync::mpsc::Sender<DebounceEventResult>, send);

/// A result of debounced events.
/// Comes with either a vec of events or an immediate error.
pub type DebounceEventResult = Result<Vec<DebouncedEvent>, Error>;

enum DebounceError {
    Notify(Error),
    Expired,
    Done,
}

struct Deadline {
    /// timeout used to compare all events against, config
    timeout: Duration,
    /// Whether to time events exactly, or batch multiple together.
    /// This reduces the amount of updates but possibly waiting longer than necessary for some events
    batch_mode: bool,
    /// next debounce deadline
    expired_after: Option<Instant>,
}
impl Deadline {
    pub fn new(timeout: Duration, batch_mode: bool) -> Self {
        Self {
            timeout,
            batch_mode,
            expired_after: None,
        }
    }

    /// Updates the deadline if none is set or when batch mode is disabled and the current deadline would miss the next event.
    /// The new deadline is calculated based on the last event update time and the debounce timeout.
    pub fn update(&mut self, last_change_time: Instant) {
        if (self.expired_after.is_some() && !self.batch_mode) || self.expired_after.is_none() {
            self.reset(last_change_time);
        }
    }

    pub fn reset(&mut self, new_start: Instant) {
        let candidate = new_start + self.timeout;
        let reset_is_allowed = self
            .expired_after
            .map_or(true, |current| current > candidate);
        if reset_is_allowed {
            self.expired_after = Some(candidate);
        }
    }

    pub fn unset(&mut self) {
        self.expired_after = None;
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.expired_after
            .map(|d| d.saturating_duration_since(Instant::now()))
    }
}

struct Instants {
    inserted: Instant,
    updated: Instant,
}

type EventCache = std::collections::HashMap<PathBuf, Instants>;

/// Retrieve a vec of debounced events, removing them if not continuous
///
/// Updates the internal tracker for the next tick
fn debounced_events(events: &mut EventCache, deadline: &mut Deadline) -> Vec<DebouncedEvent> {
    deadline.unset();

    let mut events_expired = Vec::with_capacity(events.len());
    events.retain(|path, event| {
        if event.updated.elapsed() >= deadline.timeout {
            log::trace!("debounced event: {:?}", DebouncedEventKind::Any);
            events_expired.push(DebouncedEvent::new(path.clone(), DebouncedEventKind::Any));
            false
        } else if event.inserted.elapsed() >= deadline.timeout {
            log::trace!("debounced event: {:?}", DebouncedEventKind::AnyContinuous);
            // set a new deadline, otherwise an 'AnyContinuous' will never resolve to a final 'Any' event
            deadline.update(event.updated);
            events_expired.push(DebouncedEvent::new(
                path.clone(),
                DebouncedEventKind::AnyContinuous,
            ));
            true
        } else {
            // event is neither old enough for continuous event, nor is it expired for an Any event
            deadline.update(event.updated);
            true
        }
    });
    events_expired
}

/// Debouncer guard, stops the debouncer on drop
#[derive(Debug)]
pub struct Debouncer<T: Watcher> {
    watcher: T,
    tx: Sender<Result<Event, DebounceError>>,
}

impl<T: Watcher> Debouncer<T> {
    /// Access to the internally used notify Watcher backend
    pub fn watcher(&mut self) -> &mut dyn Watcher {
        &mut self.watcher
    }
}

impl<T: Watcher> Drop for Debouncer<T> {
    fn drop(&mut self) {
        let _ = self.tx.send(Err(DebounceError::Done));
    }
}

/// Creates a new debounced watcher with custom configuration.
pub fn new_debouncer_opt<F: DebounceEventHandler, T: Watcher>(
    config: Config,
    mut event_handler: F,
) -> Result<Debouncer<T>, Error> {
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::Builder::new()
        .name("notify-rs debouncer loop".into())
        .spawn(move || {
            let mut events_by_path = EventCache::default();
            let mut deadline = Deadline::new(config.timeout, config.batch_mode);
            loop {
                match recv_with_deadline(&rx, &deadline) {
                    Ok(event) => {
                        insert_event_into_cache(&mut events_by_path, &mut deadline, event);
                    }
                    Err(DebounceError::Notify(err)) => {
                        event_handler.handle_event(Err(err));
                    }
                    Err(DebounceError::Expired) => {
                        let send_data = debounced_events(&mut events_by_path, &mut deadline);
                        if !send_data.is_empty() {
                            event_handler.handle_event(Ok(send_data));
                        }
                    }
                    Err(DebounceError::Done) => break,
                }
            }
        })?;

    let tx2 = tx.clone();

    let watcher = T::new(
        // send failure can't be handled, would need a working channel to signal that
        // also probably means that we're in the process of shutting down
        move |res: Result<_, Error>| _ = tx2.send(res.map_err(DebounceError::Notify)),
        config.notify_config,
    )?;

    Ok(Debouncer { watcher, tx })
}

fn recv_with_deadline(
    rx: &Receiver<Result<Event, DebounceError>>,
    deadline: &Deadline,
) -> Result<Event, DebounceError> {
    match deadline.remaining() {
        // wait until deadline
        Some(timeout) => rx.recv_timeout(timeout).unwrap_or_else(|err| {
            Err(match err {
                RecvTimeoutError::Timeout => DebounceError::Expired,
                RecvTimeoutError::Disconnected => DebounceError::Done,
            })
        }),
        // no deadline, wait indefinitely for event
        None => rx.recv().unwrap_or(Err(DebounceError::Done)),
    }
}

fn insert_event_into_cache(cache: &mut EventCache, deadline: &mut Deadline, event: Event) {
    log::trace!("raw event: {event:?}");
    let now = Instant::now();
    if deadline.expired_after.is_none() {
        deadline.reset(now);
    }
    for path in event.paths {
        cache
            .entry(path)
            .and_modify(|v| v.updated = now)
            .or_insert_with(|| Instants {
                inserted: now,
                updated: now,
            });
    }
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
    use notify::RecursiveMode;
    use std::fs;
    use tempfile::tempdir;

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
            let events = rx
                .recv_timeout(deadline - Instant::now())
                .expect("did not receive expected event")
                .expect("received an error");

            for event in events {
                if event.kind == DebouncedEventKind::Any
                    && (event.path == file_path || event.path == file_path.canonicalize()?)
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
