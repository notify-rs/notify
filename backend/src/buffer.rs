//! The `Buffer` utility.
//!
//! Some native platforms don't support returning a single event and keeping the rest, they just
//! give you everything they've got… but `Stream` only supports a single `Item` per `::poll_next()`
//! call, so the remaining has to be buffered somewhere. This is where.
//!
//! Furthermore, some platforms that do have a queue may behave badly when the queue overflows, for
//! example dropping all events, corrupting the queue, or aborting entirely. Instead of letting
//! that happen, `Backend` implementations for those platforms should attempt to take many events
//! from the platform queue by default, and stick them in this buffer, which has more predictable
//! behaviour. In this case, a higher than default limit should be used.
//!
//! Platforms which native queue have reasonable behaviour should not make use of this technique
//! (but using this to buffer multiple events returned from one call during poll is alright).

use crate::stream::{Error, Item};
use futures::{Poll, Stream, task::Waker};
use std::{collections::VecDeque, mem::size_of, num::NonZeroU16, pin::Pin, u16};

const U16MAX: usize = u16::max_value() as usize;

/// An internal buffer to store events obtained from native platforms.
///
/// This buffer deals in `Item`s, which are `Result<Event, Error>`. That is, both events and stream
/// errors are queued up and stored, which aligns with how the rest of Notify works.
///
/// This buffer is implemented over a `VecDeque` aka a "FIFO" queue. Items are inserted at the
/// "back" of the queue and extracted from the "front". At a set limit, further items are dropped
/// while the queue remains full, and a `Missed` error is generated. Note that this means the
/// effective limit is one less than the configured limit.
///
/// If the last item in the buffer is a `Missed` error and more items are dropped, that error's
/// drop hint count will be incremented. If a `Missed` error is received while the queue is full,
/// the received error's drop hint count will be added to the buffer's `Missed` error.
///
/// This buffer implements `Stream`, but can be used manually with additional boilerplate with the
/// `pull()` method and the rest of its API. There is a `close()` method which will irrevocably
/// close the buffer. A closed buffer will not accept any new items, but continue to serve items
/// through the `Stream` impl until it empties, at which point it will indicate that the `Stream`
/// is ended forevermore (it always behaves and there's no need to `fuse` it).

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Buffer {
    closed: bool,
    internal: VecDeque<Item>,
    limit: usize,
}

impl Buffer {
    /// Creates an empty Buffer with a configurable limit.
    pub fn new(limit: usize) -> Self {
        Self {
            closed: false,
            internal: VecDeque::new(),
            limit,
        }
    }

    /// Pushes an `Item` at the "back" of the buffer.
    ///
    /// Drops the `Item`, generating or incrementing a `Missed` event, if:
    ///
    ///  - the buffer is full, or
    ///  - the buffer is closed.
    ///
    /// If the buffer is closed _and_ has drained completely, further events are **silently**
    /// dropped, and no `Missed` error is generated.
    ///
    /// If a `Missed` error is pushed when the buffer is full, behaviour depends on the state of
    /// the buffer and the content of the missed hint, in order to retain as much information as
    /// possible while still keeping behaviour consistent.
    ///
    /// |           ×           | Buffer has Missed | No Missed yet    |
    /// |----------------------:|:-----------------:|:----------------:|
    /// | Incoming hint is None | Increment by one  | Add Missed(1)    |
    /// | Incomint hint is Some | Sum hints         | Add Missed(hint) |
    ///
    /// When full, the incoming item is always discarded, even in the case of a `Missed`: all path
    /// and attrs information is lost.
    pub fn push(&mut self, item: Item) {
        if self.closed || self.free_space().is_none() {
            // Length will only be 0 if the buffer is closed and it has drained completely.
            // At that point, no new data should be added, including Missed events.
            if self.internal.is_empty() {
                return;
            }

            let mut hint = match item {
                Err(Error::Missed(Some(h))) => h.get(),
                _ => 1,
            };

            if self.has_missed() {
                let prior_missed = self.internal.pop_back().unwrap(); // Safe because of has_missed()
                let prior_hint = match prior_missed {
                    Err(Error::Missed(Some(h))) => h.get(),
                    _ => 0, // just in case a non-buffer-added Missed is there and None
                };

                hint += prior_hint;
            }

            self.internal.push_back(Err(Error::Missed(NonZeroU16::new(hint))));
        } else {
            self.internal.push_back(item);
        }
    }

    /// Pulls an `Event` from the "front" of the buffer, if any is available.
    pub fn pull(&mut self) -> Option<Item> {
        self.internal.pop_front()
    }

    /// Irrevocably closes the buffer.
    ///
    /// A closed buffer will drop any further input, eventually draining completely.
    ///
    /// A `Missed` event _will_ be generated/incremented, unless the buffer is already drained.
    pub fn close(&mut self) {
        self.closed = true
    }

    /// Indicates whether the buffer is closed.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Returns the Event at the "front" of the buffer, if any, without consuming it.
    pub fn peek(&self) -> Option<&Item> {
        self.internal.front()
    }

    /// Indicates whether the last event is a `Missed` event.
    fn has_missed(&self) -> bool {
        match self.internal.back() {
            Some(Err(Error::Missed(_))) => true,
            _ => false,
        }
    }

    /// Indicates if and how much free space remains in the buffer.
    ///
    /// One free space is reserved for the `Missed` error at the back of the buffer. That is, the
    /// last space is always either free or filled with a `Missed` event, and in both cases this
    /// method returns `None`.
    pub fn free_space(&self) -> Option<NonZeroU16> {
        let mut len = self.internal.len();
        if !self.has_missed() {
            len += 1;
        }

        if len < self.limit {
            #[allow(clippy::cast_possible_truncation)]
            Some(NonZeroU16::new(match self.limit - len {
                hint @ 0...U16MAX => hint,
                _ => U16MAX,
            } as u16)?)
        } else {
            None
        }
    }

    /// Indicates whether the buffer is full.
    pub fn full(&self) -> bool {
        self.free_space().is_none()
    }
}

impl Stream for Buffer {
    type Item = Item;

    fn poll_next(mut self: Pin<&mut Self>, _: &Waker) -> Poll<Option<Item>> {
        match self.pull() {
            Some(item) => Poll::Ready(Some(item)),
            None => if self.closed {
                Poll::Ready(None)
            } else {
                Poll::Pending
            },
        }
    }
}

impl Default for Buffer {
    /// Creates an empty Buffer with the default limit.
    ///
    /// The default limit is computed as 16 KiB divided by the size of `Item`.
    fn default() -> Self {
        Self::new(16 * 1024 / size_of::<Item>())
    }
}
