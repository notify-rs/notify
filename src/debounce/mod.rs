#![allow(missing_docs)]

mod timer;

use super::{op, Config, Event, Result};

use self::timer::WatchTimer;
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type OperationsBuffer =
    Arc<Mutex<HashMap<PathBuf, (Option<op::Op>, Option<PathBuf>, Option<u64>)>>>;

#[derive(Clone)]
pub enum EventTx {
    Immediate {
        tx: Sender<Result<Event>>,
    },
}

impl EventTx {
    pub fn is_immediate(&self) -> bool {
        match self {
            EventTx::Immediate { .. } => true,
        }
    }

    pub fn new_immediate(tx: Sender<Result<Event>>) -> Self {
        EventTx::Immediate { tx }
    }

    pub fn configure_if_debounced(&self, _config: Config, _tx: Sender<Result<bool>>) {
    }

    pub fn send(&self, event: Result<Event>) {
        match self {
            EventTx::Immediate { ref tx } => {
                let _ = tx.send(event);
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
