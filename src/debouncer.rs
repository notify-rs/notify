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
}

impl DebounceDataInner {
    /// Retrieve a vec of debounced events, removing them if not continuous
    pub fn debounced_events(&mut self) -> Vec<DebouncedEvent> {
        let mut events_expired = Vec::with_capacity(self.d.len());
        let mut data_back = HashMap::with_capacity(self.d.len());
        // TODO: perfect fit for drain_filter https://github.com/rust-lang/rust/issues/59618
        for (k, v) in self.d.drain() {
            if v.update.elapsed() >= self.timeout {
                events_expired.push(DebouncedEvent::new(k, DebouncedEventKind::Any));
            } else if v.insert.elapsed() >= self.timeout {
                data_back.insert(k.clone(), v);
                events_expired.push(DebouncedEvent::new(k, DebouncedEventKind::AnyContinuous));
            } else {
                data_back.insert(k, v);
            }
        }
        self.d = data_back;
        events_expired
    }

    /// Add new event to debouncer cache
    pub fn add_event(&mut self, e: Event) {
        for path in e.paths.into_iter() {
            if let Some(v) = self.d.get_mut(&path) {
                v.update = Instant::now();
            } else {
                self.d.insert(path, EventData::new_any());
            }
        }
    }
}

/// Creates a new debounced watcher
pub fn new_debouncer(
    timeout: Duration,
) -> Result<(Receiver<Vec<DebouncedEvent>>, RecommendedWatcher), Error> {
    let data = DebounceData::default();

    let (tx, rx) = mpsc::channel();

    let data_c = data.clone();
    // TODO: do we want to add some ticking option ?
    let tick_div = 4;
    // TODO: use proper error kind (like InvalidConfig that requires passing a Config)
    let tick = timeout.checked_div(tick_div).ok_or_else(|| {
        Error::new(ErrorKind::Generic(format!(
            "Failed to calculate tick as {:?}/{}!",
            timeout, tick_div
        )))
    })?;
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(tick);
            let send_data;
            {
                let mut lock = data_c.lock().expect("Can't lock debouncer data!");
                send_data = lock.debounced_events();
            }
            if send_data.len() > 0 {
                // TODO: how do we efficiently detect an rx drop without sending data ?
                if tx.send(send_data).is_err() {
                    break;
                }
            }
        }
    });

    let watcher = RecommendedWatcher::new(move |e: Result<Event, Error>| {
        if let Ok(e) = e {
            let mut lock = data.lock().expect("Can't lock debouncer data!");
            lock.add_event(e);
        }
    })?;

    Ok((rx, watcher))
}
