use std::ops::{Deref, DerefMut};

#[cfg(test)]
use mock_instant::Instant;

#[cfg(not(test))]
use std::time::Instant;

use notify::Event;

/// A debounced event is emitted after a short delay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebouncedEvent {
    /// The original event.
    pub event: Event,

    /// The time at which the event occurred.
    pub time: Instant,
}

impl DebouncedEvent {
    pub fn new(event: Event, time: Instant) -> Self {
        Self { event, time }
    }
}

impl Deref for DebouncedEvent {
    type Target = Event;

    fn deref(&self) -> &Self::Target {
        &self.event
    }
}

impl DerefMut for DebouncedEvent {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.event
    }
}

impl Default for DebouncedEvent {
    fn default() -> Self {
        Self {
            event: Default::default(),
            time: Instant::now(),
        }
    }
}

impl From<Event> for DebouncedEvent {
    fn from(event: Event) -> Self {
        Self {
            event,
            time: Instant::now(),
        }
    }
}
