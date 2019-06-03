#![allow(missing_docs)]

mod timer;

use chashmap::CHashMap;
use crossbeam_channel::Sender;
use super::{event, Config, EventKind, Result};
use event::*;
use self::timer::WatchTimer;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type OperationsBuffer = Arc<CHashMap<PathBuf, (Event, Option<usize>)>>;

#[derive(Clone)]
pub enum EventTx {
    Immediate {
        tx: Sender<Result<Event>>,
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

    pub fn new_immediate(tx: Sender<Result<Event>>) -> Self {
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
        if let EventTx::Debounced { ref debounce, .. } = self {
            debounce.lock().unwrap().configure(config, tx);
        }
    }

    pub fn send(&self, event: Result<Event>) {
        match self {
            EventTx::Immediate { ref tx } => {
                let _ = tx.send(event);
            }
            EventTx::Debounced {
                ref tx,
                ref debounce,
            } => {
                match event {
                    Ok(ref e) if e.flag() == Some(Flag::Rescan) => {
                        // send rescans immediately
                        tx.send(Ok(e.clone())).ok();
                    }
                    Ok(ref e) if e.paths.is_empty() => {
                        // TODO debounce path-less events
                    }
                    Ok(e) => {
                        // debounce events per path and kind
                        debounce.lock().unwrap().event(e);
                    }
                    e @ Err(_) => {
                        // send errors immediately
                        tx.send(e).ok();
                    }
                }
            }
            EventTx::DebouncedTx { ref tx } => {
                match event {
                    Ok(ref e) if e.flag() == Some(Flag::Rescan) => {
                        // send rescans and errors immediately
                        tx.send(Ok(e.clone())).ok();
                    }
                    Ok(ref e) if e.paths.is_empty() => {
                        // TODO debounce path-less events
                    }
                    Ok(_e) => {
                        // debounce events per path and kind
                        // TODO debounce.event(e)
                    }
                    e @ Err(_) => {
                        // send errors immediately
                        tx.send(e).ok();
                    }
                }
            }
        }
    }
}

// TODO: use concurrent data structures within
// this struct to avoid the global mutex over it
#[derive(Clone)]
pub struct Debounce {
    tx: Sender<Result<Event>>,
    operations_buffer: OperationsBuffer,
    rename_path: Option<PathBuf>,
    rename_cookie: Option<usize>,
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

    fn check_partial_rename(&mut self, current_event: &Event) {
        // op == current_event.kind
        // cookie == current_event.tracker()

        let path = match current_event.paths.first() {
            Some(p) => p,
            None => return
        }.clone();

        if self.rename_path.is_none() {
            return;
        }

        // get details for the last rename event from the operations_buffer.
        // the last rename event might not be found in case the timer already fired
        // (https://github.com/passcod/notify/issues/101).
        let mut prior_event = match self.operations_buffer.get_mut(self.rename_path.as_ref().unwrap()) {
            None => return,
            Some(e) => e,
        };
        // operation == prior_event.0.kind
        // from_path == prior_event.0.paths
        // timer_id == prior_event.1

        let rename_path = self.rename_path.take().unwrap();

        let mut remove_path: Option<PathBuf> = None;

        let current_is_rename = if let EventKind::Modify(ModifyKind::Name(_)) = current_event.kind {
            true
        } else {
            false
        };

        if !(!current_is_rename
            || self.rename_cookie.is_none()
            || self.rename_cookie != current_event.tracker())
            || current_event.paths.len() > 1 // event is already a full rename
        {
            return;
        }

        if rename_path.exists() {
            match prior_event.0.kind {
                EventKind::Modify(ModifyKind::Name(_)) if prior_event.0.paths.is_empty() => {
                    // file has been moved into the watched directory
                    prior_event.0.kind = EventKind::Create(CreateKind::Any);
                    restart_timer(&mut prior_event.1, path, &mut self.timer);
                }
                EventKind::Remove(_) => {
                    // file has been moved / removed before and has now been moved into
                    // the watched directory
                    prior_event.0.kind = EventKind::Modify(ModifyKind::Any);
                    restart_timer(&mut prior_event.1, path, &mut self.timer);
                }
                _ => {
                    // this code can only be reached with fsevents because it may
                    // repeat a rename event for a file that has been renamed before
                    // (https://github.com/passcod/notify/issues/99)
                }
            }
        } else {
            match prior_event.0.kind {
                EventKind::Create(_) => {
                    // file was just created, so just remove the operations_buffer
                    // entry / no need to emit NoticeRemove because the file has just
                    // been created.

                    // ignore running timer
                    if let Some(timer_id) = prior_event.1 {
                        self.timer.ignore(timer_id);
                    }

                    // remember for deletion
                    remove_path = Some(path.clone());
                }
                EventKind::Modify(ModifyKind::Name(_)) => {
                    // file has been renamed before, change to remove event / no need
                    // to emit NoticeRemove because the file has been renamed before
                    prior_event.0.kind = EventKind::Remove(RemoveKind::Any);
                    restart_timer(&mut prior_event.1, path, &mut self.timer);
                }
                EventKind::Modify(_) => {
                    prior_event.0.kind = EventKind::Remove(RemoveKind::Any);
                    self.tx.send(Ok(Event::new(EventKind::Remove(event::RemoveKind::Any))
                                 .add_path(path.clone())
                                 .set_flag(event::Flag::Notice))).ok();
                    restart_timer(&mut prior_event.1, path, &mut self.timer);
                }
                EventKind::Remove(_) => {
                    // file has been renamed and then removed / keep write event
                    // this code can only be reached with fsevents because it may
                    // repeat a rename event for a file that has been renamed before
                    // (https://github.com/passcod/notify/issues/100)
                    restart_timer(&mut prior_event.1, path, &mut self.timer);
                }
                _ => {}
            }
        }

        self.rename_path = None;

        if let Some(path) = remove_path {
            self.operations_buffer.remove(&path);
        }
    }

    pub fn event(&mut self, current_event: Event) {
        // should be caught earlier, but let's make sure anyway.
        if current_event.flag() == Some(Flag::Rescan) {
            self.tx.send(Ok(current_event)).ok();
            return;
        }

        // TODO: multiple concurrent renames
        if self.rename_path.is_some() {
            self.check_partial_rename(&current_event);
        }

        // TODO: the rest?
        let path = match current_event.paths.first() {
            Some(p) => p,
            None => return
        };
        // op == current_event.kind
        // cookie = current_event.tracker()

        let prior_event = self.operations_buffer.get_mut(path);

        if let Some(ref prior) = prior_event {
            let (ref prev, _) = **prior;
            if current_event.kind.is_create() && (prev.kind.is_create() || prev.kind.is_modify()) {
                return;
            }

            if current_event.kind.is_remove() && prev.kind.is_remove() {
                return;
            }
        }

        if current_event.kind.is_create() {
            if let Some(mut prior) = prior_event {
                let (ref mut prev, mut timer_id) = *prior;
                if prev.kind.is_remove() {
                    // file has been removed and is now being re-created;
                    // convert this to a modify event
                    prev.kind = EventKind::Modify(ModifyKind::Any);
                    restart_timer(&mut timer_id, path.clone(), &mut self.timer);
                }
            } else {
                // set prev.kind to Create
                restart_timer(&mut None, path.clone(), &mut self.timer);
            }

            return;
        }

        if current_event.kind.is_remove() {
            if let Some(mut prior) = prior_event {
                let (ref mut prev, mut timer_id) = *prior;
                let mut any_prior = false; // TODO: convert to iterator style
                for prev_path in &prev.paths {
                    if self.operations_buffer.contains_key(prev_path) {
                        any_prior = true;
                        break;
                    }
                }

                if any_prior {
                    // A file has already been created at the same location
                    // this file has been moved from before being deleted.
                    // All events regarding this file can be ignored.

                    if let Some(timer_id) = timer_id {
                        self.timer.ignore(timer_id);
                    }

                    self.operations_buffer.remove(path);
                    if self.rename_path == Some(path.into()) {
                        self.rename_path = None;
                    }

                    return;
                }

                if prev.kind.is_create() {
                    // File was just created, so just remove the operations_buffer
                    // entry. No need to emit Remove notice because the file has
                    // just been created.

                    // ignore running timer
                    if let Some(timer_id) = timer_id {
                        self.timer.ignore(timer_id);
                    }

                    self.operations_buffer.remove(path);
                    if self.rename_path == Some(path.clone()) {
                        self.rename_path = None;
                    }
                } else if prev.kind.is_modify() {
                    // File has been renamed before, change to remove event.
                    // No need to emit Remove notice because the file has been
                    // renamed before.
                    prev.kind = EventKind::Remove(RemoveKind::Any);
                    restart_timer(&mut timer_id, path.clone(), &mut self.timer);
                }
            } else {
                self.tx.send(Ok(Event::new(EventKind::Remove(event::RemoveKind::Any))
                             .add_path(path.clone())
                             .set_flag(event::Flag::Notice))).ok();
                restart_timer(&mut None, path.clone(), &mut self.timer);
            }

            return;
        }

        if let EventKind::Modify(ModifyKind::Name(ref mode)) = current_event.kind {
            match mode {
                RenameMode::Both => {
                    // pass through this one
                },
                RenameMode::From => {
                    // from half
                },
                RenameMode::To => {
                    // to half
                },
                _ => {
                    // assume prior is `from` if extant
                    // or that current is otherwise

                    if self.rename_path.is_some()
                        && self.rename_cookie.is_some()
                        && self.rename_cookie == current_event.tracker()
                        && self.operations_buffer.contains_key(self.rename_path.as_ref().unwrap())
                    {
                        // This is the second part of a rename operation,
                        // the old path is stored in the rename_path variable

                        // unwrap is safe because rename_path is Some and op_buf contains rename_path
                        let old_path = self.rename_path.take().unwrap();
                        let (old_event, old_timer_id) = self.operations_buffer.remove(&old_path).unwrap();

                        // ignore running timer of removed operations_buffer entry
                        if let Some(old_timer_id) = old_timer_id {
                            self.timer.ignore(old_timer_id);
                        }

                        if old_event.kind.is_create() {
                            // file has just been created, so move the create event to the new path
                            if let Some(mut prior) = prior_event {
                                let (ref mut prev, mut timer_id) = *prior;
                                prev.kind = old_event.kind;
                                prev.paths = Vec::new();
                                restart_timer(&mut timer_id, path.clone(), &mut self.timer);
                            } else {
                                // create
                            }
                        } else if old_event.kind.is_modify() {
                            // file has been changed, so move the event to the new path,
                            // but keep the old event
                            if let Some(mut prior) = prior_event {
                                let (ref mut prev, mut timer_id) = *prior;
                                prev.kind = old_event.kind;
                                prev.paths = vec![old_path.clone()];
                                restart_timer(&mut timer_id, path.clone(), &mut self.timer);
                            } else {
                                // create
                            }
                        }

                        // reset
                        self.rename_path = None;
                    } else {
                        // this is the first part of a rename operation,
                        // store path for the subsequent rename event
                        self.rename_path = Some(path.clone());
                        self.rename_cookie = current_event.tracker();

                        if let Some(prior) = prior_event {
                            match *prior {
                                (Event { kind: EventKind::Create(_), .. }, mut timer_id) |
                                (Event { kind: EventKind::Modify(ModifyKind::Name(_)), .. }, mut timer_id) => {
                                    restart_timer(&mut timer_id, path.clone(), &mut self.timer);
                                },
                                _ => {}
                            }
                        } else {
                            self.tx.send(Ok(Event::new(EventKind::Remove(event::RemoveKind::Any))
                                         .add_path(path.clone())
                                         .set_flag(event::Flag::Notice))).ok();
                            restart_timer(&mut None, path.clone(), &mut self.timer);
                        }
                    }
                }
            }

            return;
        }

        if current_event.kind.is_modify() {
            if let Some(prior) = prior_event {
                let (ref prev, mut timer_id) = *prior;
                if prev.kind.is_modify() {
                    self.timer.handle_ongoing_write(&path, &self.tx);
                }

                // if file has been removed, don't send more events, else pass through
                if !prev.kind.is_remove() {
                    restart_timer(&mut timer_id, path.clone(), &mut self.timer);
                }
            } else {
                // set prev.kind to Modify
                self.tx.send(Ok(Event::new(EventKind::Modify(event::ModifyKind::Any))
                             .add_path(path.clone())
                             .set_flag(event::Flag::Notice))).ok();
                restart_timer(&mut None, path.clone(), &mut self.timer);
            }

            return;
        }

        // for anything else, just keep the old event
        if let Some(prior) = prior_event {
            let (_, mut timer_id) = *prior;
            restart_timer(&mut timer_id, path.clone(), &mut self.timer);
        }
    }
}

fn restart_timer(timer_id: &mut Option<usize>, path: PathBuf, timer: &mut WatchTimer) {
    if let Some(timer_id) = *timer_id {
        timer.ignore(timer_id);
    }

    *timer_id = Some(timer.schedule(path));
}
