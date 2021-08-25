//! Debouncer & access code
use std::{collections::HashMap, path::PathBuf, sync::{Arc, Mutex, MutexGuard, mpsc::{self, Receiver}}, time::{Duration, Instant}};

use crate::{Error, ErrorKind, Event, EventKind, RecommendedWatcher, Watcher, event::{MetadataKind, ModifyKind}};

/// Deduplicate event data entry
struct EventData {
    /// Deduplicated event
    kind: DebouncedEvent,
    /// Insertion Time
    insert: Instant,
    /// Last Update
    update: Instant,
}

/// A debounced event. Do note that any precise events are heavily platform dependent and only Any is gauranteed to work in all cases.
/// See also https://github.com/notify-rs/notify/wiki/The-Event-Guide#platform-specific-behaviour for more information.
#[derive(Eq, PartialEq, Clone)]
pub enum DebouncedEvent {
    // NoticeWrite(PathBuf),
    // NoticeRemove(PathBuf),
    /// When precise events are disabled for files
    Any,
    /// Access performed
    Access,
    /// File created
    Create,
    /// Write performed
    Write,
    /// Write performed but debounce timed out (continuous writes)
    ContinuousWrite,
    /// Metadata change like permissions
    Metadata,
    /// File deleted
    Remove,
    // Rename(PathBuf, PathBuf),
    // Rescan,
    // Error(Error, Option<PathBuf>),
}

impl From<DebouncedEvent> for EventData {
    fn from(e: DebouncedEvent) -> Self {
        let start = Instant::now();
        EventData {
            kind: e,
            insert: start.clone(),
            update: start,
        }
    }
}

type DebounceData = Arc<Mutex<DebounceDataInner>>;

#[derive(Default)]
struct DebounceDataInner {
    d: HashMap<PathBuf,EventData>,
    timeout: Duration,
}

impl DebounceDataInner {
    /// Retrieve a vec of debounced events, removing them if not continuous
    pub fn debounced_events(&mut self) -> HashMap<PathBuf, DebouncedEvent> {
        let mut events_expired = HashMap::new();
        let mut data_back = HashMap::new();
        // TODO: perfect fit for drain_filter https://github.com/rust-lang/rust/issues/59618
        for (k,v) in self.d.drain() {
            if v.update.elapsed() >= self.timeout {
                events_expired.insert(k,v.kind);
            } else if v.kind == DebouncedEvent::Write && v.insert.elapsed() >= self.timeout {
                // TODO: allow config for continuous writes reports
                data_back.insert(k.clone(),v);
                events_expired.insert(k,DebouncedEvent::ContinuousWrite);
            } else {
                data_back.insert(k,v);
            }
        }
        self.d = data_back;
        events_expired
    }

    /// Helper to insert or update EventData
    fn _insert_event(&mut self, path: PathBuf, kind: DebouncedEvent) {
        if let Some(v) = self.d.get_mut(&path) {
            // TODO: is it more efficient to take a &EventKind, compare v.kind == kind and only
            // update the v.update Instant, trading a .clone() with a compare ?
            v.update = Instant::now();
            v.kind = kind;
        } else {
            self.d.insert(path, kind.into());
        }
    }

    /// Add new event to debouncer cache
    pub fn add_event(&mut self, e: Event) {
        // TODO: handle renaming of backup files as in https://docs.rs/notify/4.0.15/notify/trait.Watcher.html#advantages
        match &e.kind {
            EventKind::Any | EventKind::Other => {
                for p in e.paths.into_iter() {
                    if let Some(existing) = self.d.get(&p) {
                        match existing.kind {
                            DebouncedEvent::Any => (),
                            _ => continue,
                        }
                    }
                    self._insert_event(p, DebouncedEvent::Any);
                }
            },
            EventKind::Access(_t) => {
                for p in e.paths.into_iter() {
                    if let Some(existing) = self.d.get(&p) {
                        match existing.kind {
                            DebouncedEvent::Any | DebouncedEvent::Access => (),
                            _ => continue,
                        }
                    }
                    self._insert_event(p, DebouncedEvent::Access);
                }
            },
            EventKind::Modify(mod_kind) => {
                let target_event = match mod_kind {
                    // ignore
                    ModifyKind::Any | ModifyKind::Other => return,
                    ModifyKind::Data(_) => DebouncedEvent::Write,
                    ModifyKind::Metadata(_) => DebouncedEvent::Metadata,
                    // TODO: handle renames
                    ModifyKind::Name(_) => return,
                };
                for p in e.paths.into_iter() {
                    if let Some(existing) = self.d.get(&p) {
                        // TODO: consider EventKind::Any on invalid configurations
                        match existing.kind {
                            DebouncedEvent::Access | DebouncedEvent::Any | DebouncedEvent::Metadata => (),
                            DebouncedEvent::Write => {
                                // don't overwrite Write with Metadata event
                                if target_event != DebouncedEvent::Write {
                                    continue;
                                }
                            }
                            _ => continue,
                        }
                    }
                    self._insert_event(p, target_event.clone());
                }
            },
            EventKind::Remove(_) => {
                // ignore previous events, override
                for p in e.paths.into_iter() {
                    self._insert_event(p, DebouncedEvent::Remove);
                }
            },
            EventKind::Create(_) => {
                // override anything except for previous Remove events
                for p in e.paths.into_iter() {
                    if let Some(e) = self.d.get(&p) {
                        if e.kind == DebouncedEvent::Remove {
                            // change to write
                            self._insert_event(p, DebouncedEvent::Write);
                            continue;
                        }
                    }
                    self._insert_event(p, DebouncedEvent::Create);
                }
            },
        }
    }
}

/// Creates a new debounced watcher
pub fn new_debouncer(timeout: Duration) -> Result<(Receiver<HashMap<PathBuf,DebouncedEvent>>,RecommendedWatcher), Error> {
    let data = DebounceData::default();
    
    let (tx,rx) = mpsc::channel();

    let data_c = data.clone();
    // TODO: do we want to add some ticking option ?
    let tick_div = 4;
    // TODO: use proper error kind (like InvalidConfig that requires passing a Config)
    let tick = timeout.checked_div(tick_div).ok_or_else(||Error::new(ErrorKind::Generic(format!("Failed to calculate tick as {:?}/{}!",timeout,tick_div))))?;
    std::thread::spawn(move ||{
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

    let watcher = RecommendedWatcher::new_immediate(move |e: Result<Event, Error>| {
        if let Ok(e) = e {
            let mut lock = data.lock().expect("Can't lock debouncer data!");
            lock.add_event(e);
        }
    })?;


    Ok((rx,watcher))
}