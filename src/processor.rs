use backend::{
    futures::{future, Async, Future, Poll}, prelude::{Capability, PathBuf}, stream,
};
use multiqueue::{BroadcastFutReceiver, BroadcastFutSender};
use std::{fmt, sync::Arc};

/// Convenience type alias for the watches currently in force.
pub type WatchesRef = Arc<Vec<PathBuf>>;

// sketch for processors:
//
// they live from the moment they're needed to the moment they're not
// often that will be the entirety of the program
// i.e. they're very much stateful
//
// prelims (processor declares):
// - whether it will operate on one backend's output or many/all
// - what capabilities it needs
// - what capabilities it provides
//
// methods:
//   - here's a new arc clone of watched paths
//   - finish up
//
// inputs:
// - stream of events
// - instruction channel
//
// outputs:
// - stream of events
// - instructions
//   - watch this
//   - unwatch this

/// Trait for processors, which post-process event streams.
pub trait Processor: fmt::Debug + Future<Item = (), Error = ()> {
    fn needs_capabilities() -> Vec<Capability>;
    fn provides_capabilities() -> Vec<Capability>;

    fn new(
        events_in: BroadcastFutReceiver<stream::Item>,
        events_out: BroadcastFutSender<stream::Item>,
        instruct_in: BroadcastFutReceiver<InstructionIn>,
        instruct_out: BroadcastFutSender<InstructionOut>,
    ) -> Result<Box<Self>, stream::Error>;
}

/// Instructions issued to a `Processor` from the manager.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstructionIn {
    UpdateWatches(WatchesRef),
    Finish,
}

/// Instructions issued from a `Processor` for the manager.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstructionOut {
    AddWatch(Vec<PathBuf>),
    RemoveWatch(Vec<PathBuf>),
}

// the processor definition lives in the notify core
// because they're really only useful with notify,
//
// whereas the backend definition is split into a crate
// because it's feasible that something could use a
// backend directly without going through notify core.

/// A sample processor that passes through every event it gets.
#[derive(Clone, Debug, Default)]
pub struct Passthru {
    watches: WatchesRef,
}

impl Processor for Passthru {
    fn needs_capabilities() -> Vec<Capability> {
        vec![]
    }
    fn provides_capabilities() -> Vec<Capability> {
        vec![]
    }

    fn new(
        _events_in: BroadcastFutReceiver<stream::Item>,
        _events_out: BroadcastFutSender<stream::Item>,
        _instruct_in: BroadcastFutReceiver<InstructionIn>,
        _instruct_out: BroadcastFutSender<InstructionOut>,
    ) -> Result<Box<Self>, stream::Error> {
        Ok(Box::new(Self::default()))
    }
}

impl Future for Passthru {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(Async::NotReady)
    }
}
