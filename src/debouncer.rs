//! Debouncer & access code
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use crate::{Error, ErrorKind, Event, RecommendedWatcher, Watcher};

/// Deduplicate event data entry
struct EventData {
    /// Insertion Time
    insert: Instant,
    /// Last Update
    update: Instant,
}

impl EventData {
    fn new_any() -> Self {
        let time = Instant::now();
        Self {
            insert: time.clone(),
            update: time,
        }
    }
}

type DebounceChannelType = Result<Vec<DebouncedEvent>, Vec<Error>>;

/// A debounced event kind.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum DebouncedEventKind {
    /// When precise events are disabled for files
    Any,
    /// Event but debounce timed out (for example continuous writes)
    AnyContinuous,
}

/// A debounced event.
///
/// Does not emit any specific event type on purpose, only distinguishes between an any event and a continuous any event.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DebouncedEvent {
    /// Event path
    pub path: PathBuf,
    /// Event kind
    pub kind: DebouncedEventKind,
}

impl DebouncedEvent {
    fn new(path: PathBuf, kind: DebouncedEventKind) -> Self {
        Self { path, kind }
    }
}

type DebounceData = Arc<Mutex<DebounceDataInner>>;

#[derive(Default)]
struct DebounceDataInner {
    d: HashMap<PathBuf, EventData>,
    timeout: Duration,
    e: Vec<crate::Error>,
}

impl DebounceDataInner {
    /// Retrieve a vec of debounced events, removing them if not continuous
    pub fn debounced_events(&mut self) -> Vec<DebouncedEvent> {
        let mut events_expired = Vec::with_capacity(self.d.len());
        let mut data_back = HashMap::with_capacity(self.d.len());
        // TODO: perfect fit for drain_filter https://github.com/rust-lang/rust/issues/59618
        for (k, v) in self.d.drain() {
            if v.update.elapsed() >= self.timeout {
                println!("normal timeout");
                events_expired.push(DebouncedEvent::new(k, DebouncedEventKind::Any));
            } else if v.insert.elapsed() >= self.timeout {
                println!("continuous");
                data_back.insert(k.clone(), v);
                events_expired.push(DebouncedEvent::new(k, DebouncedEventKind::AnyContinuous));
            } else {
                data_back.insert(k, v);
            }
        }
        self.d = data_back;
        events_expired
    }

    /// Returns all currently stored errors
    pub fn errors(&mut self) -> Vec<Error> {
        let mut v = Vec::new();
        std::mem::swap(&mut v, &mut self.e);
        v
    }

    /// Add an error entry to re-send later on
    pub fn add_error(&mut self, e: crate::Error) {
        self.e.push(e);
    }

    /// Add new event to debouncer cache
    pub fn add_event(&mut self, e: Event) {
        for path in e.paths.into_iter() {
            if let Some(v) = self.d.get_mut(&path) {
                v.update = Instant::now();
                println!("Exists");
            } else {
                self.d.insert(path, EventData::new_any());
            }
        }
    }
}

/// Creates a new debounced watcher.
/// 
/// Timeout is the amount of time after which a debounced event is emitted or a Continuous event is send, if there still are events incoming for the specific path.
/// 
/// If tick_rate is None, notify will select a tick rate that is less than the provided timeout.
pub fn new_debouncer(
    timeout: Duration,
    tick_rate: Option<Duration>,
) -> Result<(Receiver<DebounceChannelType>, RecommendedWatcher), Error> {
    let data = DebounceData::default();

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

    {
        let mut data_w = data.lock().unwrap();
        data_w.timeout = timeout;
    }

    let (tx, rx) = mpsc::channel();

    let data_c = data.clone();

    std::thread::Builder::new()
        .name("notify-rs debouncer loop".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(tick);
                let send_data;
                let errors: Vec<crate::Error>;
                {
                    let mut lock = data_c.lock().expect("Can't lock debouncer data!");
                    send_data = lock.debounced_events();
                    errors = lock.errors();
                }
                if send_data.len() > 0 {
                    // channel shut down
                    if tx.send(Ok(send_data)).is_err() {
                        break;
                    }
                }
                if errors.len() > 0 {
                    // channel shut down
                    if tx.send(Err(errors)).is_err() {
                        break;
                    }
                }
            }
        })?;

    let watcher = RecommendedWatcher::new(move |e: Result<Event, Error>| {
        let mut lock = data.lock().expect("Can't lock debouncer data!");

        match e {
            Ok(e) => lock.add_event(e),
            // can't have multiple TX, so we need to pipe that through our debouncer
            Err(e) => lock.add_error(e),
        }
    })?;

    Ok((rx, watcher))
}
