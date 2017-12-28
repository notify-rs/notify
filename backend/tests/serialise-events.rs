extern crate notify_backend as backend;

use backend::prelude::*;

#[test]
fn events_have_useful_debug_representation() {
    assert_eq!(
        format!("{:?}", EventKind::Any),
        String::from("Any")
    );

    assert_eq!(
        format!("{:?}", EventKind::Access(AccessKind::Open(AccessMode::Execute))),
        String::from("Access(Open(Execute))")
    );

    assert_eq!(
        format!("{:?}", EventKind::Remove(RemoveKind::Other("unmount".into()))),
        String::from("Remove(Other(\"unmount\"))")
    );
}
