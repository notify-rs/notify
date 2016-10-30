use super::super::{op, DebouncedEvent};

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::{Arc, Condvar, Mutex};
use std::collections::{BinaryHeap, HashSet};
use std::path::PathBuf;
use std::cmp::Ordering;

use debounce::OperationsBuffer;

enum Action {
    Schedule(ScheduledEvent),
    Ignore(u64),
}

#[derive(PartialEq, Eq)]
struct ScheduledEvent {
    id: u64,
    when: Instant,
    path: PathBuf,
}

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &ScheduledEvent) -> Ordering {
        other.when.cmp(&self.when)
    }
}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &ScheduledEvent) -> Option<Ordering> {
        other.when.partial_cmp(&self.when)
    }
}

struct ScheduleWorker {
    trigger: Arc<Condvar>,
    request_source: mpsc::Receiver<Action>,
    schedule: BinaryHeap<ScheduledEvent>,
    ignore: HashSet<u64>,
    tx: mpsc::Sender<DebouncedEvent>,
    operations_buffer: OperationsBuffer,
}

impl ScheduleWorker {
    fn new(trigger: Arc<Condvar>,
           request_source: mpsc::Receiver<Action>,
           tx: mpsc::Sender<DebouncedEvent>,
           operations_buffer: OperationsBuffer)
           -> ScheduleWorker {
        ScheduleWorker {
            trigger: trigger,
            request_source: request_source,
            schedule: BinaryHeap::new(),
            ignore: HashSet::new(),
            tx: tx,
            operations_buffer: operations_buffer,
        }
    }

    fn drain_request_queue(&mut self) {
        while let Ok(action) = self.request_source.try_recv() {
            match action {
                Action::Schedule(event) => self.schedule.push(event),
                Action::Ignore(ignore_id) => {
                    for &ScheduledEvent { ref id, .. } in &self.schedule {
                        if *id == ignore_id {
                            self.ignore.insert(ignore_id);
                            break;
                        }
                    }
                }
            }
        }
    }

    fn has_event_now(&self) -> bool {
        if let Some(event) = self.schedule.peek() {
            event.when <= Instant::now()
        } else {
            false
        }
    }

    fn fire_event(&mut self) {
        if let Some(ScheduledEvent { id, path, .. }) = self.schedule.pop() {
            if !self.ignore.remove(&id) {
                if let Ok(ref mut op_buf) = self.operations_buffer.lock() {
                    if let Some((op, from_path, _)) = op_buf.remove(&path) {
                        let is_partial_rename = from_path.is_none();
                        if let Some(from_path) = from_path {
                            self.tx.send(DebouncedEvent::Rename(from_path, path.clone())).unwrap();
                        }
                        let message = match op {
                            Some(op::CREATE) => Some(DebouncedEvent::Create(path)),
                            Some(op::WRITE) => Some(DebouncedEvent::Write(path)),
                            Some(op::CHMOD) => Some(DebouncedEvent::Chmod(path)),
                            Some(op::REMOVE) => Some(DebouncedEvent::Remove(path)),
                            Some(op::RENAME) if is_partial_rename => {
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
        }
    }

    fn duration_until_next_event(&self) -> Option<Duration> {
        self.schedule.peek().map(|event| {
            let now = Instant::now();
            if event.when <= now {
                Duration::from_secs(0)
            } else {
                event.when.duration_since(now)
            }
        })
    }

    fn run(&mut self) {
        let m = Mutex::new(());

        // Unwrapping is safe because the mutex can't be poisoned,
        // since we just created it.
        let mut g = m.lock().unwrap();

        loop {
            self.drain_request_queue();

            while self.has_event_now() {
                self.fire_event();
            }

            let wait_duration = self.duration_until_next_event();

            // Unwrapping is safe because the mutex can't be poisoned,
            // since we haven't shared it with another thread.
            g = if let Some(wait_duration) = wait_duration {
                self.trigger.wait_timeout(g, wait_duration).unwrap().0
            } else {
                self.trigger.wait(g).unwrap()
            };
        }
    }
}

pub struct WatchTimer {
    counter: u64,
    schedule_tx: mpsc::Sender<Action>,
    trigger: Arc<Condvar>,
    delay: Duration,
}

impl WatchTimer {
    pub fn new(tx: mpsc::Sender<DebouncedEvent>,
               operations_buffer: OperationsBuffer,
               delay: Duration)
               -> WatchTimer {
        let (schedule_tx, schedule_rx) = mpsc::channel();
        let trigger = Arc::new(Condvar::new());

        let trigger_worker = trigger.clone();
        thread::spawn(move || {
            ScheduleWorker::new(trigger_worker, schedule_rx, tx, operations_buffer).run();
        });

        WatchTimer {
            counter: 0,
            schedule_tx: schedule_tx,
            trigger: trigger,
            delay: delay,
        }
    }

    pub fn schedule(&mut self, path: PathBuf) -> u64 {
        self.counter = self.counter.wrapping_add(1);

        self.schedule_tx
            .send(Action::Schedule(ScheduledEvent {
                id: self.counter,
                when: Instant::now() + self.delay,
                path: path,
            }))
            .expect("Failed to send a request to the global scheduling worker");

        self.trigger.notify_one();

        self.counter
    }

    pub fn ignore(&self, id: u64) {
        self.schedule_tx
            .send(Action::Ignore(id))
            .expect("Failed to send a request to the global scheduling worker");
    }
}
