#![allow(missing_docs)]

mod timer;

use super::{event, op, Config, Event, EventKind, RawEvent, Result};

use self::timer::WatchTimer;
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type OperationsBuffer =
    Arc<Mutex<HashMap<PathBuf, (Option<op::Op>, Option<PathBuf>, Option<u64>)>>>;

#[derive(Clone)]
pub enum EventTx {
    Immediate {
        tx: Sender<RawEvent>,
    },
    DebouncedTx {
        tx: Sender<Result<Event>>,
    },
    Debounced {
        tx: Sender<Result<Event>>,
        debounce: Arc<Mutex<Debounce>>,
    },
}

impl EventTx {
    pub fn is_immediate(&self) -> bool {
        match self {
            EventTx::Immediate { .. } => true,
            _ => false,
        }
    }

    pub fn new_immediate(tx: Sender<RawEvent>) -> Self {
        EventTx::Immediate { tx }
    }

    pub fn new_debounced_tx(tx: Sender<Result<Event>>) -> Self {
        EventTx::DebouncedTx { tx }
    }

    pub fn new_debounced(tx: Sender<Result<Event>>, debounce: Debounce) -> Self {
        EventTx::Debounced {
            tx,
            debounce: Arc::new(Mutex::new(debounce)),
        }
    }

    pub fn debounced_tx(&self) -> Self {
        match self {
            EventTx::Debounced { ref tx, .. } => Self::new_debounced_tx(tx.clone()),
            _ => unreachable!(),
        }
    }

    pub fn configure_if_debounced(&self, config: Config, tx: Sender<Result<bool>>) {
        match self {
            EventTx::Debounced { ref debounce, .. } => {
                debounce.lock().unwrap().configure(config, tx);
            }
            _ => {}
        }
    }

    pub fn send(&self, event: RawEvent) {
        match self {
            EventTx::Immediate { ref tx } => {
                let _ = tx.send(event);
            }
            EventTx::Debounced {
                ref tx,
                ref debounce,
            } => {
                match (event.path, event.op, event.cookie) {
                    (None, Ok(op::Op::RESCAN), None) => {
                        tx.send(Ok(
                            Event::new(EventKind::Other).set_flag(event::Flag::Rescan)
                        ))
                        .ok();
                    }
                    (Some(path), Ok(op), cookie) => {
                        debounce.lock().unwrap().event(path, op, cookie);
                    }
                    (None, Ok(_op), _cookie) => {
                        // TODO debounce path-less events
                    }
                    (Some(path), Err(e), _) => {
                        tx.send(Err(e.set_paths(vec![path]))).ok();
                    }
                    (None, Err(e), _) => {
                        tx.send(Err(e)).ok();
                    }
                }
            }
            EventTx::DebouncedTx { ref tx } => {
                match (event.path, event.op, event.cookie) {
                    (None, Ok(op::Op::RESCAN), None) => {
                        tx.send(Ok(
                            Event::new(EventKind::Other).set_flag(event::Flag::Rescan)
                        ))
                        .ok();
                    }
                    (Some(_path), Ok(_op), _cookie) => {
                        // TODO debounce.event(_path, _op, _cookie);
                    }
                    (None, Ok(_op), _cookie) => {
                        // TODO debounce path-less events
                    }
                    (Some(path), Err(e), _) => {
                        tx.send(Err(e.set_paths(vec![path]))).ok();
                    }
                    (None, Err(e), _) => {
                        tx.send(Err(e)).ok();
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct Debounce {
    tx: Sender<Result<Event>>,
    operations_buffer: OperationsBuffer,
    rename_path: Option<PathBuf>,
    rename_cookie: Option<u32>,
    timer: WatchTimer,
}

impl Debounce {
    pub fn new(delay: Duration, tx: Sender<Result<Event>>) -> Debounce {
        let operations_buffer: OperationsBuffer = Arc::default();

        // spawns new thread
        let timer = WatchTimer::new(tx.clone(), operations_buffer.clone(), delay);

        Debounce {
            tx,
            operations_buffer,
            rename_path: None,
            rename_cookie: None,
            timer,
        }
    }

    pub fn configure(&mut self, config: Config, tx: Sender<Result<bool>>) {
        tx.send(match config {
            Config::OngoingEvents(c) => self.timer.set_ongoing(c),
            _ => Ok(false),
        })
        .expect("configuration channel disconnected");
    }

    fn check_partial_rename(&mut self, path: PathBuf, op: op::Op, cookie: Option<u32>) {
        if let Ok(mut op_buf) = self.operations_buffer.lock() {
            // the previous event was a rename event, but this one isn't; something went wrong
            let mut remove_path: Option<PathBuf> = None;

            // get details for the last rename event from the operations_buffer.
            // the last rename event might not be found in case the timer already fired
            // (https://github.com/passcod/notify/issues/101).
            if let Some(&mut (ref mut operation, ref mut from_path, ref mut timer_id)) =
                op_buf.get_mut(self.rename_path.as_ref().unwrap())
            {
                if op != op::Op::RENAME
                    || self.rename_cookie.is_none()
                    || self.rename_cookie != cookie
                {
                    if self.rename_path.as_ref().unwrap().exists() {
                        match *operation {
                            Some(op::Op::RENAME) if from_path.is_none() => {
                                // file has been moved into the watched directory
                                *operation = Some(op::Op::CREATE);
                                restart_timer(timer_id, path, &mut self.timer);
                            }
                            Some(op::Op::REMOVE) => {
                                // file has been moved removed before and has now been moved into
                                // the watched directory
                                *operation = Some(op::Op::WRITE);
                                restart_timer(timer_id, path, &mut self.timer);
                            }
                            _ => {
                                // this code can only be reached with fsevents because it may
                                // repeat a rename event for a file that has been renamed before
                                // (https://github.com/passcod/notify/issues/99)
                            }
                        }
                    } else {
                        match *operation {
                            Some(op::Op::CREATE) => {
                                // file was just created, so just remove the operations_buffer
                                // entry / no need to emit NoticeRemove because the file has just
                                // been created.

                                // ignore running timer
                                if let Some(timer_id) = *timer_id {
                                    self.timer.ignore(timer_id);
                                }

                                // remember for deletion
                                remove_path = Some(path);
                            }
                            Some(op::Op::WRITE) | // change to remove event
                            Some(op::Op::METADATA) => { // change to remove event
                                *operation = Some(op::Op::REMOVE);
                                self.tx.send(Ok(Event::new(EventKind::Remove(event::RemoveKind::Any))
                                             .add_path(path.clone())
                                             .set_flag(event::Flag::Notice))).ok();
                                restart_timer(timer_id, path, &mut self.timer);
                            }
                            Some(op::Op::RENAME) => {

                                // file has been renamed before, change to remove event / no need
                                // to emit NoticeRemove because the file has been renamed before
                                *operation = Some(op::Op::REMOVE);
                                restart_timer(timer_id, path, &mut self.timer);
                            }
                            Some(op::Op::REMOVE) => {

                                // file has been renamed and then removed / keep write event
                                // this code can only be reached with fsevents because it may
                                // repeat a rename event for a file that has been renamed before
                                // (https://github.com/passcod/notify/issues/100)
                                restart_timer(timer_id, path, &mut self.timer);
                            }
                            // CLOSE_WRITE and RESCAN aren't tracked by operations_buffer
                            _ => {
                                unreachable!();
                            }
                        }
                    }
                    self.rename_path = None;
                }
            }

            if let Some(path) = remove_path {
                op_buf.remove(&path);
            }
        }
    }

    pub fn event(&mut self, path: PathBuf, mut op: op::Op, cookie: Option<u32>) {
        if op.contains(op::Op::RESCAN) {
            self.tx
                .send(Ok(
                    Event::new(EventKind::Other).set_flag(event::Flag::Rescan)
                ))
                .ok();
        }

        if self.rename_path.is_some() {
            self.check_partial_rename(path.clone(), op, cookie);
        }

        if let Ok(mut op_buf) = self.operations_buffer.lock() {
            if let Some(&(ref operation, _, _)) = op_buf.get(&path) {
                op = remove_repeated_events(op, operation);
            } else if op.contains(op::Op::CREATE | op::Op::REMOVE) {
                if path.exists() {
                    op.remove(op::Op::REMOVE);
                } else {
                    op.remove(op::Op::CREATE);
                }
            }

            if op.contains(op::Op::CREATE) {
                let &mut (ref mut operation, _, ref mut timer_id) =
                    op_buf.entry(path.clone()).or_insert((None, None, None));
                match *operation {
                    // file can't be created twice
                    Some(op::Op::CREATE) |

                    // file can't be written to before being created
                    Some(op::Op::WRITE) |

                    // file can't be changed before being created
                    Some(op::Op::METADATA) |

                    // file can't be renamed to before being created
                    // (repetitions are removed anyway),
                    // but with fsevents everything is possible
                    Some(op::Op::RENAME) => {}

                    // file has been removed and is now being re-created;
                    // convert this to a write event
                    Some(op::Op::REMOVE) => {
                        *operation = Some(op::Op::WRITE);
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                    }

                    // operations_buffer entry didn't exist
                    None => {
                        *operation = Some(op::Op::CREATE);
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                    }

                    _ => { unreachable!(); }
                }
            }

            if op.contains(op::Op::WRITE) {
                let &mut (ref mut operation, _, ref mut timer_id) =
                    op_buf.entry(path.clone()).or_insert((None, None, None));
                match *operation {
                    // keep create event / no need to emit NoticeWrite because
                    // the file has just been created
                    Some(op::Op::CREATE) |

                    // keep write event / not need to emit NoticeWrite because
                    // it already was a write event
                    Some(op::Op::WRITE) => {
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                        self.timer.handle_ongoing_write(&path, &self.tx);
                    }

                    // upgrade to write event
                    Some(op::Op::METADATA) |

                    // file has been renamed before, upgrade to write event
                    Some(op::Op::RENAME) |

                    // operations_buffer entry didn't exist
                    None => {
                        *operation = Some(op::Op::WRITE);
                        self.tx.send(Ok(Event::new(EventKind::Modify(event::ModifyKind::Any))
                                     .add_path(path.clone())
                                     .set_flag(event::Flag::Notice))).ok();
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                    }

                    // writing to a deleted file is impossible,
                    // but with fsevents everything is possible
                    Some(op::Op::REMOVE) => {}

                    _ => { unreachable!(); }
                }
            }

            if op.contains(op::Op::METADATA) {
                let &mut (ref mut operation, _, ref mut timer_id) =
                    op_buf.entry(path.clone()).or_insert((None, None, None));
                match *operation {
                    // keep create event
                    Some(op::Op::CREATE) |

                    // keep write event
                    Some(op::Op::WRITE) |

                    // keep metadata event
                    Some(op::Op::METADATA) => { restart_timer(timer_id, path.clone(), &mut self.timer); }

                    // file has been renamed before, upgrade to metadata event
                    Some(op::Op::RENAME) |

                    // operations_buffer entry didn't exist
                    None => {
                        *operation = Some(op::Op::METADATA);
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                    }

                    // changing a deleted file is impossible,
                    // but with fsevents everything is possible
                    Some(op::Op::REMOVE) => {}

                    _ => { unreachable!(); }
                }
            }

            if op.contains(op::Op::RENAME) {
                // unwrap is safe because rename_path is Some
                if self.rename_path.is_some()
                    && self.rename_cookie.is_some()
                    && self.rename_cookie == cookie
                    && op_buf.contains_key(self.rename_path.as_ref().unwrap())
                {
                    // This is the second part of a rename operation, the old path is stored in the
                    // rename_path variable.

                    // unwrap is safe because rename_path is Some and op_buf contains rename_path
                    let (from_operation, from_from_path, from_timer_id) =
                        op_buf.remove(self.rename_path.as_ref().unwrap()).unwrap();

                    // ignore running timer of removed operations_buffer entry
                    if let Some(from_timer_id) = from_timer_id {
                        self.timer.ignore(from_timer_id);
                    }

                    // if the file has been renamed before, use original name as from_path
                    let use_from_path = from_from_path.or(self.rename_path.clone());

                    let &mut (ref mut operation, ref mut from_path, ref mut timer_id) =
                        op_buf.entry(path.clone()).or_insert((None, None, None));

                    match from_operation {
                        // file has just been created, so move the create event to the new path
                        Some(op::Op::CREATE) => {
                            *operation = from_operation;
                            *from_path = None;
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // file has been written to, so move the event to the new path, but keep
                        // the write event
                        Some(op::Op::WRITE) |

                        // file has been changed, so move the event to the new path, but keep the
                        // metadata event
                        Some(op::Op::METADATA) |

                        // file has been renamed before, so move the event to the new path and
                        // update the from_path
                        Some(op::Op::RENAME) => {
                            *operation = from_operation;
                            *from_path = use_from_path;
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // file can't be renamed after being removed,
                        // but with fsevents everything is possible
                        Some(op::Op::REMOVE) => {}

                        _ => { unreachable!(); }
                    }

                    // reset the rename_path
                    self.rename_path = None;
                } else {
                    // this is the first part of a rename operation,
                    // store path for the subsequent rename event
                    self.rename_path = Some(path.clone());
                    self.rename_cookie = cookie;

                    let &mut (ref mut operation, _, ref mut timer_id) =
                        op_buf.entry(path.clone()).or_insert((None, None, None));
                    match *operation {
                        // keep create event / no need to emit NoticeRemove because
                        // the file has just been created
                        Some(op::Op::CREATE) |

                        // file has been renamed before, so
                        // keep rename event / no need to emit NoticeRemove because
                        // the file has been renamed before
                        Some(op::Op::RENAME) => {
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // keep write event
                        Some(op::Op::WRITE) |

                        // keep metadata event
                        Some(op::Op::METADATA) => {
                            self.tx.send(Ok(Event::new(EventKind::Remove(event::RemoveKind::Any))
                                         .add_path(path.clone())
                                         .set_flag(event::Flag::Notice))).ok();
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // operations_buffer entry didn't exist
                        None => {
                            *operation = Some(op::Op::RENAME);
                            self.tx.send(Ok(Event::new(EventKind::Remove(event::RemoveKind::Any))
                                         .add_path(path.clone())
                                         .set_flag(event::Flag::Notice))).ok();
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // renaming a deleted file should be impossible,
                        // but with fsevents everything is possible
                        // (https://github.com/passcod/notify/issues/101)
                        Some(op::Op::REMOVE) => {}

                        _ => { unreachable!(); }
                    }
                }
            }

            if op.contains(op::Op::REMOVE) {
                let mut remove_path: Option<PathBuf> = None;
                {
                    if let Some(&(_, ref from_path, ref timer_id)) = op_buf.get(&path) {
                        if let Some(ref from_path) = *from_path {
                            if op_buf.contains_key(from_path) {
                                // a file has already been created at the same location this file
                                // has been moved from before being deleted / all events
                                // regarding this file can be ignored

                                // ignore running timer
                                if let Some(timer_id) = *timer_id {
                                    self.timer.ignore(timer_id);
                                }

                                // remember for deletion
                                remove_path = Some(path.clone());
                            }
                        }
                    }

                    let &mut (ref mut operation, _, ref mut timer_id) =
                        op_buf.entry(path.clone()).or_insert((None, None, None));

                    if remove_path.is_none() {
                        match *operation {
                            // file was just created, so just remove the operations_buffer entry / no
                            // need to emit NoticeRemove because the file has just been created
                            Some(op::Op::CREATE) => {
                                // ignore running timer
                                if let Some(timer_id) = *timer_id {
                                    self.timer.ignore(timer_id);
                                }

                                // remember for deletion
                                remove_path = Some(path.clone());
                            }

                            // change to remove event
                            Some(op::Op::WRITE) |

                            // change to remove event
                            Some(op::Op::METADATA) |

                            // operations_buffer entry didn't exist
                            None => {
                                *operation = Some(op::Op::REMOVE);
                                self.tx.send(Ok(Event::new(EventKind::Remove(event::RemoveKind::Any))
                                             .add_path(path.clone())
                                             .set_flag(event::Flag::Notice))).ok();
                                restart_timer(timer_id, path.clone(), &mut self.timer);
                            }

                            // file has been renamed before, change to remove event /
                            // no need to emit NoticeRemove because the file has been renamed before
                            Some(op::Op::RENAME) => {
                                *operation = Some(op::Op::REMOVE);
                                restart_timer(timer_id, path.clone(), &mut self.timer);
                            }

                            // multiple remove events are possible if the file/directory
                            // is itself watched and in a watched directory
                            Some(op::Op::REMOVE) => {}

                            _ => { unreachable!(); }
                        }
                    }
                }
                if let Some(path) = remove_path {
                    op_buf.remove(&path);
                    if self.rename_path == Some(path) {
                        self.rename_path = None;
                    }
                }
            }
        }
    }
}

fn remove_repeated_events(mut op: op::Op, prev_op: &Option<op::Op>) -> op::Op {
    if let Some(prev_op) = *prev_op {
        if prev_op.intersects(op::Op::CREATE | op::Op::WRITE | op::Op::METADATA | op::Op::RENAME) {
            op.remove(op::Op::CREATE);
        }

        if prev_op.contains(op::Op::REMOVE) {
            op.remove(op::Op::REMOVE);
        }

        if prev_op.contains(op::Op::RENAME) && op & !op::Op::RENAME != op::Op::empty() {
            op.remove(op::Op::RENAME);
        }
    }
    op
}

fn restart_timer(timer_id: &mut Option<u64>, path: PathBuf, timer: &mut WatchTimer) {
    if let Some(timer_id) = *timer_id {
        timer.ignore(timer_id);
    }
    *timer_id = Some(timer.schedule(path));
}
