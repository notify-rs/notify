use crate::{event, op, Error, Event, EventKind, Result};
use chashmap::CHashMap;
use crossbeam_channel::Sender;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{
    atomic::{self, AtomicBool},
    Arc, Condvar, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

use debounce::OperationsBuffer;

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
    tx: Sender<Event>,
    operations_buffer: OperationsBuffer,
    stopped: Arc<AtomicBool>,
    ongoing_writes: Arc<CHashMap<PathBuf, Instant>>,
}

impl ScheduleWorker {
    fn fire_due_events(&self, now: Instant) -> Option<Instant> {
        let mut events = self.events.lock().unwrap();
        while let Some(event) = events.pop_front() {
            if event.when <= now {
                self.fire_event(event)
            } else {
                // not due yet, put it back
                let next_when = event.when;
                events.push_front(event);
                return Some(next_when);
            }
        }
        None
    }

    fn fire_event(&self, ev: ScheduledEvent) {
        let ScheduledEvent { path, .. } = ev;
        if let Ok(ref mut op_buf) = self.operations_buffer.lock() {
            if let Some((op, from_path, _)) = op_buf.remove(&path) {
                let is_partial_rename = from_path.is_none();
                if let Some(from_path) = from_path {
                    self.tx
                        .send(
                            Event::new(EventKind::Modify(event::ModifyKind::Name(
                                event::RenameMode::Both,
                            )))
                            .add_path(from_path)
                            .add_path(path.clone()),
                        )
                        .ok();
                }
                let message = match op {
                    Some(op::Op::CREATE) => {
                        Some(Event::new(EventKind::Create(event::CreateKind::Any)).add_path(path))
                    }
                    Some(op::Op::WRITE) => {
                        self.ongoing_writes.remove(&path);
                        Some(Event::new(EventKind::Modify(event::ModifyKind::Any)).add_path(path))
                    }
                    Some(op::Op::METADATA) => Some(
                        Event::new(EventKind::Modify(event::ModifyKind::Metadata(
                            event::MetadataKind::Any,
                        )))
                        .add_path(path),
                    ),
                    Some(op::Op::REMOVE) => {
                        self.ongoing_writes.remove(&path);
                        Some(Event::new(EventKind::Remove(event::RemoveKind::Any)).add_path(path))
                    }
                    Some(op::Op::RENAME) if is_partial_rename => {
                        if path.exists() {
                            Some(
                                Event::new(EventKind::Create(event::CreateKind::Any))
                                    .add_path(path),
                            )
                        } else {
                            Some(
                                Event::new(EventKind::Remove(event::RemoveKind::Any))
                                    .add_path(path),
                            )
                        }
                    }
                    _ => None,
                };
                if let Some(m) = message {
                    let _ = self.tx.send(m);
                }
            } else {
                // TODO error!("path not found in operations_buffer: {}", path.display())
            }
        }
    }

    fn run(&mut self) {
        let m = Mutex::new(());

        // Unwrapping is safe because the mutex can't be poisoned,
        // since we just created it.
        let mut g = m.lock().unwrap();

        loop {
            let now = Instant::now();
            let next_when = self.fire_due_events(now);

            if self.stopped.load(atomic::Ordering::SeqCst) {
                break;
            }

            // Unwrapping is safe because the mutex can't be poisoned,
            // since we haven't shared it with another thread.
            g = if let Some(next_when) = next_when {
                // wait for stop notification or timeout to send next event
                self.stop_trigger
                    .wait_timeout(g, next_when - now)
                    .unwrap()
                    .0
            } else {
                // no pending events
                // wait for new event, to check when it should be send and then wait to send it
                self.new_event_trigger.wait(g).unwrap()
            };
        }
    }
}

#[derive(Clone)]
pub struct WatchTimer {
    counter: u64,
    new_event_trigger: Arc<Condvar>,
    stop_trigger: Arc<Condvar>,
    delay: Duration,
    events: Arc<Mutex<VecDeque<ScheduledEvent>>>,
    stopped: Arc<AtomicBool>,
    ongoing_writes: Arc<CHashMap<PathBuf, Instant>>,
    ongoing_delay: Option<Duration>,
}

impl WatchTimer {
    pub fn new(
        tx: Sender<Event>,
        operations_buffer: OperationsBuffer,
        delay: Duration,
    ) -> WatchTimer {
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let new_event_trigger = Arc::new(Condvar::new());
        let stop_trigger = Arc::new(Condvar::new());
        let stopped = Arc::new(AtomicBool::new(false));
        let ongoing_writes = Arc::new(CHashMap::new());

        let worker_new_event_trigger = new_event_trigger.clone();
        let worker_stop_trigger = stop_trigger.clone();
        let worker_events = events.clone();
        let worker_stopped = stopped.clone();
        let worker_ongoing_writes = ongoing_writes.clone();
        thread::spawn(move || {
            ScheduleWorker {
                new_event_trigger: worker_new_event_trigger,
                stop_trigger: worker_stop_trigger,
                events: worker_events,
                tx,
                operations_buffer,
                stopped: worker_stopped,
                ongoing_writes: worker_ongoing_writes,
            }
            .run();
        });

        WatchTimer {
            counter: 0,
            new_event_trigger,
            stop_trigger,
            delay,
            events,
            stopped,
            ongoing_writes,
            ongoing_delay: None,
        }
    }

    pub fn set_ongoing_writes(&mut self, delay: Option<Duration>) -> Result<bool> {
        if let Some(delay) = delay {
            if delay > self.delay {
                return Err(Error::InvalidConfigValue);
            }
        } else if self.ongoing_delay.is_some() {
            // Reset the current ongoing state when disabling
            self.ongoing_writes.clear();
        }

        self.ongoing_delay = delay;
        Ok(true)
    }

    pub fn handle_ongoing_write(&self, path: &PathBuf, tx: &Sender<Event>) {
        if let Some(delay) = self.ongoing_delay {
            self.ongoing_writes.upsert(
                path.clone(),
                || Instant::now() + delay,
                |fire_at| {
                    if fire_at <= &mut Instant::now() {
                        tx.send(
                            Event::new(EventKind::Modify(event::ModifyKind::Any))
                                .add_path(path.clone())
                                .set_flag(event::Flag::Ongoing),
                        )
                        .ok();
                        *fire_at = Instant::now() + delay;
                    }
                },
            );
        }
    }

    pub fn schedule(&mut self, path: PathBuf) -> u64 {
        self.counter = self.counter.wrapping_add(1);

        self.events.lock().unwrap().push_back(ScheduledEvent {
            id: self.counter,
            when: Instant::now() + self.delay,
            path: path,
        });

        self.new_event_trigger.notify_one();

        self.counter
    }

    pub fn ignore(&self, id: u64) {
        let mut events = self.events.lock().unwrap();
        let index = events.iter().rposition(|e| e.id == id);
        if let Some(index) = index {
            events.remove(index);
        }
    }
}

impl Drop for WatchTimer {
    fn drop(&mut self) {
        self.stopped.store(true, atomic::Ordering::SeqCst);
        self.stop_trigger.notify_one();
        self.new_event_trigger.notify_one();
    }
}
