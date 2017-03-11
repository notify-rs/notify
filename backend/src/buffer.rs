use futures::{Async, Poll};
use std::collections::VecDeque;
use super::event::Event;
use super::stream::{Error, Item};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Buffer {
    closed: bool,
    internal: VecDeque<Event>,
    limit: usize
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer::new_with_limit(16384)
    }

    pub fn new_with_limit(limit: usize) -> Buffer {
        Buffer { closed: false, internal: VecDeque::new(), limit: limit }
    }

    pub fn push(&mut self, event: Event) {
        if self.closed { return }
        if self.free_space().is_none() { return }
        self.internal.push_back(event)
    }

    pub fn pull(&mut self) -> Option<Event> {
        self.internal.pop_front()
    }

    pub fn poll(&mut self) -> Poll<Option<Item>, Error> {
        Ok(match self.pull() {
            Some(item) => Async::Ready(Some(item)),
            None => match self.closed {
                true => Async::Ready(None),
                false => Async::NotReady
            }
        })
    }

    pub fn close(&mut self) {
        self.closed = true
    }

    pub fn closed(&self) -> bool {
        self.closed
    }

    pub fn peek(&self) -> Option<&Event> {
        self.internal.front()
    }

    pub fn free_space(&self) -> Option<usize> {
        let len = self.internal.len();
        if len < self.limit {
            Some(self.limit - len)
        } else {
            None
        }
    }

    pub fn full(&self) -> bool {
        self.free_space().is_none()
    }
}
