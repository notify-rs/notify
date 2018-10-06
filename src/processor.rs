use backend::{
    prelude::{Capability, PathBuf}, stream,
};
use multiqueue::{BroadcastFutReceiver, BroadcastFutSender};
use std::{fmt, sync::Arc};

/// Convenience type alias for the watches currently in force.
pub type WatchesRef = Arc<Vec<PathBuf>>;
// should be a Set, not a Vec

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
pub trait Processor: fmt::Debug {
    fn needs_capabilities() -> Vec<Capability>;
    fn provides_capabilities() -> Vec<Capability>;

    fn new(
        events_in: BroadcastFutReceiver<stream::Item>,
        events_out: BroadcastFutSender<stream::Item>,
        instruct: BroadcastFutSender<Instruction>,
        // consider:
        // instruct_in: Receiver<Enum { UpdateWatches(Arc<Vec>), Finish }>
        // instead of the methods, then treat the entire thing as a Future
    ) -> Result<Box<Self>, stream::Error>;
    fn spawn(&mut self); // -> Future

    fn update_watches(&mut self, paths: WatchesRef) -> Result<(), stream::Error>;
    fn finish(&mut self);
}

/// Instructions issued from a `Processor` for the manager.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Instruction {
    AddWatch(Vec<PathBuf>),
    RemoveWatch(Vec<PathBuf>),
}

// the processor definition lives in the notify core
// because they're really only useful with notify,
// whereas the backend definition is split into a crate
// because it's feasible that something could use a
// backend directly without going through notify core.

/// A sample processor that passes through every event it gets.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Passthru {
    watches: WatchesRef,
}

impl Processor for Passthru {
    fn needs_capabilities() -> Vec<Capability> { vec![] }
    fn provides_capabilities() -> Vec<Capability> { vec![] }

    fn new(
        _events_in: BroadcastFutReceiver<stream::Item>,
        _events_out: BroadcastFutSender<stream::Item>,
        _instruct: BroadcastFutSender<Instruction>,
    ) -> Result<Box<Self>, stream::Error> {
        Ok(Box::new(Self::default()))
    }

    fn spawn(&mut self) {}
    fn update_watches(&mut self, paths: WatchesRef) -> Result<(), stream::Error> {
        self.watches = paths;
        Ok(())
    }

    fn finish(&mut self) {}
}
