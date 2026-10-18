use std::path::PathBuf;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A debounced event kind.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum DebouncedEventKind {
    /// No precise events
    Any,
    /// Event but debounce timed out (for example continuous writes)
    AnyContinuous,
}

/// A debounced event.
///
/// Does not emit any specific event type on purpose, only distinguishes between an any event and a continuous any event.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DebouncedEvent {
    /// Event path
    pub path: PathBuf,
    /// Event kind
    pub kind: DebouncedEventKind,
}

impl DebouncedEvent {
    #[inline(always)]
    #[must_use]
    pub fn new(path: PathBuf, kind: DebouncedEventKind) -> Self {
        Self { path, kind }
    }
}

/// A notification from the mini debouncer.
///
/// Handle [`Self::Rescan`] before applying path or event-kind filters to [`Self::Events`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DebouncedEventBatch {
    /// A batch of debounced path changes.
    Events(Vec<DebouncedEvent>),
    /// Events were missed or can no longer describe the current filesystem state reliably.
    ///
    /// Refresh any cached filesystem state for all watched paths. This notification applies
    /// to the entire watcher, regardless of any paths attached to the original event.
    /// It is delivered without waiting for the debounce timeout. Pending path changes
    /// remain queued and are delivered according to their usual debounce deadlines.
    Rescan,
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialize_and_deserialize_batches() {
        for (batch, json) in [
            (DebouncedEventBatch::Rescan, json!("Rescan")),
            (
                DebouncedEventBatch::Events(vec![
                    DebouncedEvent::new("file.txt".into(), DebouncedEventKind::Any),
                    DebouncedEvent::new("active.txt".into(), DebouncedEventKind::AnyContinuous),
                ]),
                json!({"Events": [
                    {"path": "file.txt", "kind": "Any"},
                    {"path": "active.txt", "kind": "AnyContinuous"},
                ]}),
            ),
        ] {
            assert_eq!(serde_json::to_value(&batch).unwrap(), json);
            assert_eq!(
                serde_json::from_value::<DebouncedEventBatch>(json).unwrap(),
                batch,
            );
        }
    }
}
