use super::super::{op, DebouncedEvent};

use std::ops::DerefMut;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::{collections::VecDeque, sync::MutexGuard};

use debounce::{OperationsBuffer, OperationsBufferInner};

#[derive(PartialEq, Eq)]
struct ScheduledEvent {
    id: u64,
    when: Instant,
    path: PathBuf,
}

#[derive(Default)]
struct WorkerSharedState {
    is_stopped: bool,
    events: VecDeque<ScheduledEvent>,
}

struct ScheduleWorker {
    state: Arc<(Mutex<WorkerSharedState>, Condvar)>,
    tx: mpsc::Sender<DebouncedEvent>,
    operations_buffer: OperationsBuffer,
}

impl ScheduleWorker {
    fn fire_due_events<'a>(
        &'a self,
        now: Instant,
        state: MutexGuard<'a, WorkerSharedState>,
    ) -> (Option<Instant>, MutexGuard<'a, WorkerSharedState>) {
        // simple deadlock avoidance loop.
        let mut state = Some(state);
        let (mut state, mut op_buf) = loop {
            let state = state.take().unwrap_or_else(|| self.state.0.lock().unwrap());

            // To avoid deadlock, we do a `try_lock`, and on `WouldBlock`, we unlock the
            // events Mutex, and retry after yielding.
            match self.operations_buffer.try_lock() {
                Ok(op_buf) => break (state, op_buf),
                Err(::std::sync::TryLockError::Poisoned { .. }) => return (None, state),
                Err(::std::sync::TryLockError::WouldBlock) => {
                    // drop the lock before yielding to give other threads a chance to complete
                    // their work.
                    drop(state);
                    ::std::thread::yield_now();
                }
            }
        };
        while let Some(event) = state.events.pop_front() {
            if event.when <= now {
                self.fire_event(event, &mut op_buf)
            } else {
                // not due yet, put it back
                let next_when = event.when;
                state.events.push_front(event);
                return (Some(next_when), state);
            }
        }
        (None, state)
    }

    fn fire_event(
        &self,
        ev: ScheduledEvent,
        op_buf: &mut impl DerefMut<Target = OperationsBufferInner>,
    ) {
        let ScheduledEvent { path, .. } = ev;
        if let Some((op, from_path, _)) = op_buf.remove(&path) {
            let is_partial_rename = from_path.is_none();
            if let Some(from_path) = from_path {
                self.tx
                    .send(DebouncedEvent::Rename(from_path, path.clone()))
                    .unwrap();
            }
            let message = match op {
                Some(op::Op::CREATE) => Some(DebouncedEvent::Create(path)),
                Some(op::Op::WRITE) => Some(DebouncedEvent::Write(path)),
                Some(op::Op::CHMOD) => Some(DebouncedEvent::Chmod(path)),
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

    fn run(&mut self) {
        let mut state = self.state.0.lock().unwrap();
        loop {
            let now = Instant::now();
            let (next_when, state_out) = self.fire_due_events(now, state);
            state = state_out;

            if state.is_stopped {
                break;
            }

            state = if let Some(next_when) = next_when {
                // wait for stop notification or timeout to send next event
                self.state.1.wait_timeout(state, next_when - now).unwrap().0
            } else {
                // no pending events
                // wait for new event, to check when it should be send and then wait to send it
                self.state.1.wait(state).unwrap()
            };
        }
    }
}

pub struct WatchTimer {
    counter: u64,
    state: Arc<(Mutex<WorkerSharedState>, Condvar)>,
    delay: Duration,
}

impl WatchTimer {
    pub fn new(
        tx: mpsc::Sender<DebouncedEvent>,
        operations_buffer: OperationsBuffer,
        delay: Duration,
    ) -> WatchTimer {
        let state = Arc::new((Mutex::new(WorkerSharedState::default()), Condvar::new()));

        let worker_state = state.clone();
        thread::spawn(move || {
            ScheduleWorker {
                state: worker_state,
                tx,
                operations_buffer,
            }
            .run();
        });

        WatchTimer {
            counter: 0,
            state,
            delay,
        }
    }

    pub fn schedule(&mut self, path: PathBuf) -> u64 {
        self.counter = self.counter.wrapping_add(1);

        {
            let mut state = self.state.0.lock().unwrap();
            state.events.push_back(ScheduledEvent {
                id: self.counter,
                when: Instant::now() + self.delay,
                path,
            });
        }
        self.state.1.notify_one();

        self.counter
    }

    pub fn ignore(&self, id: u64) {
        let mut state = self.state.0.lock().unwrap();
        let index = state.events.iter().rposition(|e| e.id == id);
        if let Some(index) = index {
            state.events.remove(index);
        }
    }
}

impl Drop for WatchTimer {
    fn drop(&mut self) {
        {
            let mut state = self.state.0.lock().unwrap();
            state.is_stopped = true;
        }
        self.state.1.notify_one();
    }
}
