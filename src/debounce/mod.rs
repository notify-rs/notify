#![allow(missing_docs)]

mod timer;

use super::{op, RawEvent, DebouncedEvent};

use self::timer::WatchTimer;

use std::sync::mpsc;
use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type OperationsBuffer = Arc<Mutex<HashMap<PathBuf,
                                              (Option<op::Op>, Option<PathBuf>, Option<u64>)>>>;

pub enum EventTx {
    Raw { tx: mpsc::Sender<RawEvent> },
    Debounced {
        tx: mpsc::Sender<DebouncedEvent>,
        debounce: Debounce,
    },
    DebouncedTx { tx: mpsc::Sender<DebouncedEvent> },
}

impl EventTx {
    pub fn send(&mut self, event: RawEvent) {
        match *self {
            EventTx::Raw { ref tx } => {
                let _ = tx.send(event);
            }
            EventTx::Debounced { ref tx, ref mut debounce } => {
                match (event.path, event.op, event.cookie) {
                    (None, Ok(op::RESCAN), None) => {
                        let _ = tx.send(DebouncedEvent::Rescan);
                    }
                    (Some(path), Ok(op), cookie) => {
                        debounce.event(path, op, cookie);
                    }
                    (None, Ok(_op), _cookie) => {
                        // TODO panic!("path is None: {:?} ({:?})", _op, _cookie);
                    }
                    (path, Err(e), _) => {
                        let _ = tx.send(DebouncedEvent::Error(e, path));
                    }
                }
            }
            EventTx::DebouncedTx { ref tx } => {
                match (event.path, event.op, event.cookie) {
                    (None, Ok(op::RESCAN), None) => {
                        let _ = tx.send(DebouncedEvent::Rescan);
                    }
                    (Some(_path), Ok(_op), _cookie) => {
                        // TODO debounce.event(_path, _op, _cookie);
                    }
                    (None, Ok(_op), _cookie) => {
                        // TODO panic!("path is None: {:?} ({:?})", _op, _cookie);
                    }
                    (path, Err(e), _) => {
                        let _ = tx.send(DebouncedEvent::Error(e, path));
                    }
                }
            }
        }
    }
}

pub struct Debounce {
    tx: mpsc::Sender<DebouncedEvent>,
    operations_buffer: OperationsBuffer,
    rename_path: Option<PathBuf>,
    rename_cookie: Option<u32>,
    timer: WatchTimer,
}

impl Debounce {
    pub fn new(delay: Duration, tx: mpsc::Sender<DebouncedEvent>) -> Debounce {
        let operations_buffer: OperationsBuffer = Arc::new(Mutex::new(HashMap::new()));

        // spawns new thread
        let timer = WatchTimer::new(tx.clone(), operations_buffer.clone(), delay);

        Debounce {
            tx: tx,
            operations_buffer: operations_buffer,
            rename_path: None,
            rename_cookie: None,
            timer: timer,
        }
    }

    fn check_partial_rename(&mut self, path: PathBuf, op: op::Op, cookie: Option<u32>) {
        if let Ok(mut op_buf) = self.operations_buffer.lock() {
            // the previous event was a rename event, but this one isn't; something went wrong
            let mut remove_path: Option<PathBuf> = None;

            // get details for the last rename event from the operations_buffer.
            // the last rename event might not be found in case the timer already fired
            // (https://github.com/passcod/notify/issues/101).
            if let Some(&mut (ref mut operation, ref mut from_path, ref mut timer_id)) =
                op_buf.get_mut(self.rename_path.as_ref().unwrap()) {
                if op != op::RENAME || self.rename_cookie.is_none() ||
                   self.rename_cookie != cookie {
                    if self.rename_path.as_ref().unwrap().exists() {
                        match *operation {
                            Some(op::RENAME) if from_path.is_none() => {
                                // file has been moved into the watched directory
                                *operation = Some(op::CREATE);
                                restart_timer(timer_id, path, &mut self.timer);
                            }
                            Some(op::REMOVE) => {
                                // file has been moved removed before and has now been moved into
                                // the watched directory
                                *operation = Some(op::WRITE);
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
                            Some(op::CREATE) => {
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
                            Some(op::WRITE) | // change to remove event
                            Some(op::CHMOD) => { // change to remove event
                                *operation = Some(op::REMOVE);
                                let _ = self.tx.send(DebouncedEvent::NoticeRemove(path.clone()));
                                restart_timer(timer_id, path, &mut self.timer);
                            }
                            Some(op::RENAME) => {

                                // file has been renamed before, change to remove event / no need
                                // to emit NoticeRemove because the file has been renamed before
                                *operation = Some(op::REMOVE);
                                restart_timer(timer_id, path, &mut self.timer);
                            }
                            Some(op::REMOVE) => {

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
        if op.contains(op::RESCAN) {
            let _ = self.tx.send(DebouncedEvent::Rescan);
        }

        if self.rename_path.is_some() {
            self.check_partial_rename(path.clone(), op, cookie);
        }

        if let Ok(mut op_buf) = self.operations_buffer.lock() {
            if let Some(&(ref operation, _, _)) = op_buf.get(&path) {
                op = remove_repeated_events(op, operation);
            } else if op.contains(op::CREATE | op::REMOVE) {
                if path.exists() {
                    op.remove(op::REMOVE);
                } else {
                    op.remove(op::CREATE);
                }
            }

            if op.contains(op::CREATE) {
                let &mut (ref mut operation, _, ref mut timer_id) = op_buf.entry(path.clone())
                    .or_insert((None, None, None));
                match *operation {
                    // file can't be created twice
                    Some(op::CREATE) |

                    // file can't be written to before being created
                    Some(op::WRITE) |

                    // file can't be changed before being created
                    Some(op::CHMOD) |

                    // file can't be renamed to before being created
                    // (repetitions are removed anyway),
                    // but with fsevents everything is possible
                    Some(op::RENAME) => {}

                    // file has been removed and is now being re-created;
                    // convert this to a write event
                    Some(op::REMOVE) => {
                        *operation = Some(op::WRITE);
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                    }

                    // operations_buffer entry didn't exist
                    None => {
                        *operation = Some(op::CREATE);
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                    }

                    _ => { unreachable!(); }
                }
            }

            if op.contains(op::WRITE) {
                let &mut (ref mut operation, _, ref mut timer_id) = op_buf.entry(path.clone())
                    .or_insert((None, None, None));
                match *operation {
                    // keep create event / no need to emit NoticeWrite because
                    // the file has just been created
                    Some(op::CREATE) |

                    // keep write event / not need to emit NoticeWrite because
                    // it already was a write event
                    Some(op::WRITE) => {
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                    }

                    // upgrade to write event
                    Some(op::CHMOD) |

                    // file has been renamed before, upgrade to write event
                    Some(op::RENAME) |

                    // operations_buffer entry didn't exist
                    None => {
                        *operation = Some(op::WRITE);
                        let _ = self.tx.send(DebouncedEvent::NoticeWrite(path.clone()));
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                    }

                    // writing to a deleted file is impossible,
                    // but with fsevents everything is possible
                    Some(op::REMOVE) => {}

                    _ => { unreachable!(); }
                }
            }

            if op.contains(op::CHMOD) {
                let &mut (ref mut operation, _, ref mut timer_id) = op_buf.entry(path.clone())
                    .or_insert((None, None, None));
                match *operation {
                    // keep create event
                    Some(op::CREATE) |

                    // keep write event
                    Some(op::WRITE) |

                    // keep chmod event
                    Some(op::CHMOD) => { restart_timer(timer_id, path.clone(), &mut self.timer); }

                    // file has been renamed before, upgrade to chmod event
                    Some(op::RENAME) |

                    // operations_buffer entry didn't exist
                    None => {
                        *operation = Some(op::CHMOD);
                        restart_timer(timer_id, path.clone(), &mut self.timer);
                    }

                    // changing a deleted file is impossible,
                    // but with fsevents everything is possible
                    Some(op::REMOVE) => {}

                    _ => { unreachable!(); }
                }
            }

            if op.contains(op::RENAME) {
                // unwrap is safe because rename_path is Some
                if self.rename_path.is_some() && self.rename_cookie.is_some() &&
                   self.rename_cookie == cookie &&
                   op_buf.contains_key(self.rename_path.as_ref().unwrap()) {
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
                        Some(op::CREATE) => {
                            *operation = from_operation;
                            *from_path = None;
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // file has been written to, so move the event to the new path, but keep
                        // the write event
                        Some(op::WRITE) |

                        // file has been changed, so move the event to the new path, but keep the
                        // chmod event
                        Some(op::CHMOD) |

                        // file has been renamed before, so move the event to the new path and
                        // update the from_path
                        Some(op::RENAME) => {
                            *operation = from_operation;
                            *from_path = use_from_path;
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // file can't be renamed after beeing removed,
                        // but with fsevents everything is possible
                        Some(op::REMOVE) => {}

                        _ => { unreachable!(); }
                    }

                    // reset the rename_path
                    self.rename_path = None;
                } else {
                    // this is the first part of a rename operation,
                    // store path for the subsequent rename event
                    self.rename_path = Some(path.clone());
                    self.rename_cookie = cookie;

                    let &mut (ref mut operation, _, ref mut timer_id) = op_buf.entry(path.clone())
                        .or_insert((None, None, None));
                    match *operation {
                        // keep create event / no need to emit NoticeRemove because
                        // the file has just been created
                        Some(op::CREATE) |

                        // file has been renamed before, so
                        // keep rename event / no need to emit NoticeRemove because
                        // the file has been renamed before
                        Some(op::RENAME) => {
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // keep write event
                        Some(op::WRITE) |

                        // keep chmod event
                        Some(op::CHMOD) => {
                            let _ = self.tx.send(DebouncedEvent::NoticeRemove(path.clone()));
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // operations_buffer entry didn't exist
                        None => {
                            *operation = Some(op::RENAME);
                            let _ = self.tx.send(DebouncedEvent::NoticeRemove(path.clone()));
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // renaming a deleted file should be impossible,
                        // but with fsevents everything is possible
                        // (https://github.com/passcod/notify/issues/101)
                        Some(op::REMOVE) => {}

                        _ => { unreachable!(); }
                    }
                }
            }

            if op.contains(op::REMOVE) {
                let mut remove_path: Option<PathBuf> = None;
                {
                    let &mut (ref mut operation, _, ref mut timer_id) = op_buf.entry(path.clone())
                        .or_insert((None, None, None));

                    match *operation {
                        // file was just created, so just remove the operations_buffer entry / no
                        // need to emit NoticeRemove because the file has just been created
                        Some(op::CREATE) => {
                            // ignore running timer
                            if let Some(timer_id) = *timer_id {
                                self.timer.ignore(timer_id);
                            }

                            // remember for deletion
                            remove_path = Some(path.clone());
                        }

                        // change to remove event
                        Some(op::WRITE) |

                        // change to remove event
                        Some(op::CHMOD) |

                        // operations_buffer entry didn't exist
                        None => {
                            *operation = Some(op::REMOVE);
                            let _ = self.tx.send(DebouncedEvent::NoticeRemove(path.clone()));
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // file has been renamed before, change to remove event /
                        // no need to emit NoticeRemove because the file has been renamed before
                        Some(op::RENAME) => {
                            *operation = Some(op::REMOVE);
                            restart_timer(timer_id, path.clone(), &mut self.timer);
                        }

                        // multiple remove events are possible if the file/directory
                        // is itself watched and in a watched directory
                        Some(op::REMOVE) => {}

                        _ => { unreachable!(); }
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
        if prev_op.intersects(op::CREATE | op::WRITE | op::CHMOD | op::RENAME) {
            op.remove(op::CREATE);
        }

        if prev_op.contains(op::REMOVE) {
            op.remove(op::REMOVE);
        }

        if prev_op.contains(op::RENAME) && op & !op::RENAME != op::Op::empty() {
            op.remove(op::RENAME);
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
