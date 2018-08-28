//! The `Buffer` utility.

use super::event::Event;
use super::stream::{Error, Item};
use futures::{Async, Poll};
use std::collections::VecDeque;

/// An internal buffer to store events obtained from native interfaces.
///
/// Some native APIs don't support returning a single event and keeping the rest, they just give
/// you everything they've got… but `Stream` only supports a single `Item` per `::poll()` call, so
/// the remaining has to be buffered somewhere. This is where.
///
/// Furthermore, native APIs that do have a queue generally behave badly/annoyingly when the queue
/// overflows. Instead of letting that happen, `Backend` implementations should attempt to take
/// many events from the native queue by default, and stick them in this buffer, which has more
/// predictable behaviour.
///
/// This buffer is implemented over a `VecDeque` aka a "FIFO" queue. Items are inserted at the
/// "back" of the queue and extracted from the "front". At a set limit (by default 16384 items),
/// further items are silently dropped while the queue remains full.
///
/// This buffer has a `poll()` method which obeys `Stream` semantics, and can be used instead of
/// additional boilerplate over the `pull()` method. The buffer also has a `close()` method which
/// will irrevocably close the buffer. A closed buffer will not accept any new items, but continue
/// to serve items through the `poll()` method until it empties, at which point it will indicate
/// that the `Stream` is ended.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Buffer {
    closed: bool,
    internal: VecDeque<Event>,
    limit: usize,
}

impl Buffer {
    /// Creates an empty Buffer with the default limit (16384).
    pub fn new() -> Self {
        Self::new_with_limit(16384)
    }

    /// Creates an empty Buffer with a configurable limit.
    pub fn new_with_limit(limit: usize) -> Self {
        Self {
            closed: false,
            internal: VecDeque::new(),
            limit,
        }
    }

    /// Pushes an `Event` at the "back" of the buffer.
    ///
    /// Silently drops the `Event` if:
    ///
    ///  - the buffer is full, or
    ///  - the buffer is closed.
    pub fn push(&mut self, event: Event) {
        if self.closed {
            return;
        }
        if self.free_space().is_none() {
            return;
        }
        self.internal.push_back(event)
    }

    /// Pulls an `Event` from the "front" of the buffer, if any is available.
    pub fn pull(&mut self) -> Option<Event> {
        self.internal.pop_front()
    }

    /// Polls the buffer for an `Event`, compatible with `Stream` semantics.
    ///
    ///  - If an Event is available, return `Ready(Some(event))`;
    ///  - if no Event is available and the buffer is closed, return `Ready(None)`;
    ///  - otherwise, return `NotReady`.
    pub fn poll(&mut self) -> Poll<Option<Item>, Error> {
        Ok(match self.pull() {
            Some(item) => Async::Ready(Some(item)),
            None => if self.closed {
                Async::Ready(None)
            } else {
                Async::NotReady
            },
        })
    }

    /// Irrevocably closes the buffer.
    ///
    /// A closed buffer will silently drop any further input, eventually draining completely.
    pub fn close(&mut self) {
        self.closed = true
    }

    /// Indicates whether the buffer is closed.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Returns the Event at the "front" of the buffer, if any, without consuming it.
    pub fn peek(&self) -> Option<&Event> {
        self.internal.front()
    }

    /// Indicates if and how much free space remains in the buffer.
    pub fn free_space(&self) -> Option<usize> {
        let len = self.internal.len();
        if len < self.limit {
            Some(self.limit - len)
        } else {
            None
        }
    }

    /// Indicates whether the buffer is full.
    pub fn full(&self) -> bool {
        self.free_space().is_none()
    }
}
