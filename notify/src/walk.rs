use crate::ErrorKind;

/// What a failed watch installation means for the rest of a recursive walk.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WalkFailure {
    /// Nothing was lost, so the walk carries on.
    Skip,
    /// This path errored, but the rest of the tree will still be walked.
    Report,
    /// The watch limit prevents further additions, so the walk ends here.
    Abort,
}

impl WalkFailure {
    pub(crate) fn of(kind: &ErrorKind) -> Self {
        match kind {
            // TOCTOU: an entry can disappear between being listed and being watched.
            ErrorKind::PathNotFound => Self::Skip,
            ErrorKind::MaxFilesWatch => Self::Abort,
            _ => Self::Report,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn walk_failure_policy() {
        assert_eq!(WalkFailure::of(&ErrorKind::PathNotFound), WalkFailure::Skip);
        assert_eq!(
            WalkFailure::of(&ErrorKind::MaxFilesWatch),
            WalkFailure::Abort
        );
        assert_eq!(
            WalkFailure::of(&ErrorKind::Io(io::ErrorKind::PermissionDenied.into())),
            WalkFailure::Report
        );
        assert_eq!(
            WalkFailure::of(&ErrorKind::Generic("symbolic link loop".into())),
            WalkFailure::Report
        );
    }
}
