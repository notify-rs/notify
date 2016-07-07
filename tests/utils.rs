extern crate notify;
extern crate time;

use notify::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

#[cfg(not(target_os="windows"))]
const TIMEOUT_S: f64 = 0.1;
#[cfg(target_os="windows")]
const TIMEOUT_S: f64 = 3.0; // windows can take a while

pub fn recv_events(rx: Receiver<Event>) ->  Vec<(PathBuf, Op)> {
    let deadline = time::precise_time_s() + TIMEOUT_S;

    let mut evs: Vec<(PathBuf, Op)> = Vec::new();

    while time::precise_time_s() < deadline {
        match rx.try_recv() {
            Ok(Event{path: Some(path), op: Ok(op)}) => {
                evs.push((path, op));
            },
            Ok(Event{path: None, ..})  => (),
            Ok(Event{op: Err(e), ..}) => panic!("unexpected event err: {:?}", e),
            Err(TryRecvError::Empty) => (),
            Err(e) => panic!("unexpected channel err: {:?}", e)
        }
    }
    evs
}

// FSEvent tends to emit events multiple times and aggregate events,
// so just check that all expected events arrive for each path,
// and make sure the paths are in the correct order
#[allow(dead_code)]
pub fn inflate_events(input: Vec<(PathBuf, Op)>) -> Vec<(PathBuf, Op)> {
    let mut output = Vec::new();
    let mut path = None;
    let mut ops = Op::empty();
    for (e_p, e_op) in input {
        let p = match path {
            Some(p) => p,
            None => e_p.clone()
        };
        if p == e_p {
            // ops |= e_op;
            ops = Op::from_bits_truncate(ops.bits() | e_op.bits());
        } else {
            output.push((p, ops));
            ops = e_op;
        }
        path = Some(e_p);
    }
    if let Some(p) = path {
        output.push((p, ops));
    }
    output
}

#[cfg(not(target_os="macos"))]
pub fn canonicalize(path: &Path) -> PathBuf {
    path.to_owned()
}

#[cfg(target_os="macos")]
pub fn canonicalize(path: &Path) -> PathBuf {
    path.canonicalize().expect("failed to canonalize path").to_owned()
}
