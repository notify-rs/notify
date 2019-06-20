use chashmap::CHashMap;
use crossbeam_channel::Sender;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{
    atomic::{self, AtomicBool},
    Arc, Condvar, Mutex,
};
use std::time::{Duration, Instant};
use crate::{Event, Result};
use crate::debounce::OperationsBuffer;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScheduledEvent {
    id: u64,
    when: Instant,
    path: PathBuf,
}

#[derive(Clone)]
struct ScheduleWorker {
    new_event_trigger: Arc<Condvar>,
    stop_trigger: Arc<Condvar>,
    events: Arc<Mutex<VecDeque<ScheduledEvent>>>,
    tx: Sender<Result<Event>>,
    operations_buffer: OperationsBuffer,
    stopped: Arc<AtomicBool>,
    ongoing_events: Arc<CHashMap<PathBuf, Instant>>,
}

#[derive(Clone)]
pub struct WatchTimer {
    counter: u64,
    new_event_trigger: Arc<Condvar>,
    stop_trigger: Arc<Condvar>,
    delay: Duration,
    events: Arc<Mutex<VecDeque<ScheduledEvent>>>,
    stopped: Arc<AtomicBool>,
    ongoing_events: Arc<CHashMap<PathBuf, Instant>>,
    ongoing_delay: Option<Duration>,
}

impl Drop for WatchTimer {
    fn drop(&mut self) {
        self.stopped.store(true, atomic::Ordering::SeqCst);
        self.stop_trigger.notify_one();
        self.new_event_trigger.notify_one();
    }
}
