//! The types that are only available on Unix.

use std::{
    io,
    os::unix::io::RawFd
};

use mio::{
    event::Evented,
    unix::EventedFd,
    Poll as MioPoll,
    PollOpt as MioPollOpt,
    Ready as MioReady,
    Token as MioToken
};

/// An `Evented` FD that owns its FD.
///
/// This is a convenience type to be used to return an [`EventedFd`] when doing so would not
/// usually be possible due to lifetimes. Notably, this type does not do any kind of lifecycle
/// events related to the FD, that is the responsibility of the Backend.
///
/// [`EventedFd`]: https://docs.rs/mio/0.6/mio/unix/struct.EventedFd.html
#[derive(Clone, Copy, Debug)]
pub struct OwnedEventedFd(pub RawFd);

impl Evented for OwnedEventedFd {
    fn register(&self, poll: &MioPoll, token: MioToken, interest: MioReady, opts: MioPollOpt) -> io::Result<()> {
        EventedFd(&self.0).register(poll, token, interest, opts)
    }

    fn reregister(&self, poll: &MioPoll, token: MioToken, interest: MioReady, opts: MioPollOpt) -> io::Result<()> {
        EventedFd(&self.0).reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &MioPoll) -> io::Result<()> {
        EventedFd(&self.0).deregister(poll)
    }
}
