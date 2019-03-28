use super::super::{op, DebouncedEvent};

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{
    atomic::{self, AtomicBool},
    Arc, Condvar, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

use debounce::OperationsBuffer;

#[derive(PartialEq, Eq)]
struct ScheduledEvent {
    id: u64,
    when: Instant,
    path: PathBuf,
}

struct ScheduleWorker {
    new_event_trigger: Arc<Condvar>,
    stop_trigger: Arc<Condvar>,
    events: Arc<Mutex<VecDeque<ScheduledEvent>>>,
    tx: mpsc::Sender<DebouncedEvent>,
    operations_buffer: OperationsBuffer,
    stopped: Arc<AtomicBool>,
    worker_ongoing_write_event: Arc<Mutex<Option<(Instant, PathBuf)>>>,
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
                        .send(DebouncedEvent::Rename(from_path, path.clone()))
                        .unwrap();
                }
                let message = match op {
                    Some(op::Op::CREATE) => Some(DebouncedEvent::Create(path)),
                    Some(op::Op::WRITE) => {
                        //disable ongoing_write
                        let mut ongoing_write_event = self.worker_ongoing_write_event.lock().unwrap();
                        *ongoing_write_event = None;
                        Some(DebouncedEvent::Write(path))
                    },
                    Some(op::Op::METADATA) => Some(DebouncedEvent::Chmod(path)),
                    Some(op::Op::REMOVE) => Some(DebouncedEvent::Remove(path)),
                    Some(op::Op::RENAME) if is_partial_rename => {
                        if path.exists() {
                            Some(DebouncedEvent::Create(path))
                        } else {
                            Some(DebouncedEvent::Remove(path))
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

pub struct WatchTimer {
    counter: u64,
    new_event_trigger: Arc<Condvar>,
    stop_trigger: Arc<Condvar>,
    delay: Duration,
    events: Arc<Mutex<VecDeque<ScheduledEvent>>>,
    stopped: Arc<AtomicBool>,
    pub ongoing_write_event: Arc<Mutex<Option<(Instant, PathBuf)>>>,
    pub ongoing_write_duration: Option<Duration>,
}

impl WatchTimer {
    pub fn new(
        tx: mpsc::Sender<DebouncedEvent>,
        operations_buffer: OperationsBuffer,
        delay: Duration,
    ) -> WatchTimer {
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let new_event_trigger = Arc::new(Condvar::new());
        let stop_trigger = Arc::new(Condvar::new());
        let stopped = Arc::new(AtomicBool::new(false));

        let worker_new_event_trigger = new_event_trigger.clone();
        let worker_stop_trigger = stop_trigger.clone();
        let worker_events = events.clone();
        let worker_stopped = stopped.clone();
        let ongoing_write_event = Arc::new(Mutex::new(None));
        let worker_ongoing_write_event = ongoing_write_event.clone();
        thread::spawn(move || {
            ScheduleWorker {
                new_event_trigger: worker_new_event_trigger,
                stop_trigger: worker_stop_trigger,
                events: worker_events,
                tx,
                operations_buffer,
                stopped: worker_stopped,
                worker_ongoing_write_event,
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
            ongoing_write_event,
            ongoing_write_duration: None,
        }
    }

    pub fn set_ongoing_write_duration(&mut self, duration: Option<Duration>) {
        self.ongoing_write_duration = duration;
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
