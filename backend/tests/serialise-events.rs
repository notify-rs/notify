extern crate notify_backend as backend;

use backend::prelude::*;

#[test]
fn events_have_useful_debug_representation() {
    assert_eq!(format!("{:?}", EventKind::Any), String::from("Any"));

    assert_eq!(
        format!(
            "{:?}",
            EventKind::Access(AccessKind::Open(AccessMode::Execute))
        ),
        String::from("Access(Open(Execute))")
    );

    let mut attrs = AnyMap::new();
    attrs.insert(event::Info("unmount".into()));

    assert_eq!(
        format!(
            "{:?}",
            Event {
                kind: EventKind::Remove(RemoveKind::Other),
                path: Some("/example".into()),
                attrs
            }
        ),
        String::from(
            "Event { kind: Remove(Other), path: Some(\"/example\"), attr:tracker: None, attr:info: Some(\"unmount\"), attr:source: None }"
        )
    );
}
