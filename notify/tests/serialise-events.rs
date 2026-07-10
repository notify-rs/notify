// This file is dual-licensed under the Artistic License 2.0 as per the
// LICENSE.ARTISTIC file, and the Creative Commons Zero 1.0 license.

use notify::event::*;
#[cfg(feature = "serde")]
use serde_json::json;

#[test]
fn events_are_debuggable() {
    assert_eq!(format!("{:?}", EventKind::Any), String::from("Any"));

    assert_eq!(
        format!(
            "{:?}",
            EventKind::Access(AccessKind::Open(AccessMode::Execute))
        ),
        String::from("Access(Open(Execute))")
    );

    let mut attrs = EventAttributes::new();
    attrs.set_info("unmount");
    attrs.set_flag(Flag::Rescan);

    assert_eq!(
        format!(
            "{:?}",
            Event {
                kind: EventKind::Remove(RemoveKind::Other),
                paths: vec!["/example".into()],
                attrs
            }
        ),
        String::from(
            "Event { kind: Remove(Other), paths: [\"/example\"], attr:tracker: None, attr:flag: Some(Rescan), attr:info: Some(\"unmount\"), attr:source: None }"
        )
    );
}

#[cfg(feature = "serde")]
#[test]
fn events_are_serializable() {
    assert_eq!(json!(EventKind::Any), json!({ "type": "any" }));

    assert_eq!(json!(EventKind::Other), json!({ "type": "other" }));

    assert_eq!(
        json!(Event {
            kind: EventKind::Access(AccessKind::Open(AccessMode::Execute)),
            paths: Vec::new(),
            attrs: EventAttributes::new(),
        }),
        json!({
            "type": "access",
            "kind": "open",
            "mode": "execute",
            "paths": [],
            "attrs": {},
        })
    );

    let mut attrs = EventAttributes::new();
    attrs.set_info("unmount");

    assert_eq!(
        json!(Event {
            kind: EventKind::Remove(RemoveKind::Other),
            paths: vec!["/example".into()],
            attrs: attrs.clone(),
        }),
        json!({
            "type": "remove",
            "kind": "other",
            "paths": ["/example"],
            "attrs": { "info": "unmount" }
        }),
        "{:#?} != {:#?}",
        json!(Event {
            kind: EventKind::Remove(RemoveKind::Other),
            paths: vec!["/example".into()],
            attrs: attrs.clone(),
        }),
        json!({
            "type": "remove",
            "kind": "other",
            "paths": ["/example"],
            "attrs": { "info": "unmount" }
        }),
    );
}

#[cfg(feature = "serde")]
#[test]
fn events_are_deserializable() {
    assert_eq!(
        serde_json::from_str::<EventKind>(r#"{ "type": "any" }"#).unwrap(),
        EventKind::Any
    );

    assert_eq!(
        serde_json::from_str::<EventKind>(r#"{ "type": "other" }"#).unwrap(),
        EventKind::Other
    );

    assert_eq!(
        serde_json::from_str::<Event>(
            r#"{
        "type": "access",
        "kind": "open",
        "mode": "execute",
        "paths": [],
        "attrs": {}
    }"#
        )
        .unwrap(),
        Event {
            kind: EventKind::Access(AccessKind::Open(AccessMode::Execute)),
            paths: Vec::new(),
            attrs: EventAttributes::new(),
        }
    );

    let mut attrs = EventAttributes::new();
    attrs.set_info("unmount");

    assert_eq!(
        serde_json::from_str::<Event>(
            r#"{
        "type": "remove",
        "kind": "other",
        "paths": ["/example"],
        "attrs": { "info": "unmount" }
    }"#
        )
        .unwrap(),
        Event {
            kind: EventKind::Remove(RemoveKind::Other),
            paths: vec!["/example".into()],
            attrs
        }
    );
}

#[cfg(feature = "serde")]
#[test]
fn access_events_are_serializable() {
    assert_eq!(
        json!(EventKind::Access(AccessKind::Any)),
        json!({ "type": "access", "kind": "any" })
    );

    assert_eq!(
        json!(EventKind::Access(AccessKind::Read)),
        json!({ "type": "access", "kind": "read" })
    );

    assert_eq!(
        json!(EventKind::Access(AccessKind::Open(AccessMode::Any))),
        json!({ "type": "access", "kind": "open", "mode": "any" })
    );

    assert_eq!(
        json!(EventKind::Access(AccessKind::Open(AccessMode::Execute))),
        json!({ "type": "access", "kind": "open", "mode": "execute" })
    );

    assert_eq!(
        json!(EventKind::Access(AccessKind::Open(AccessMode::Read))),
        json!({ "type": "access", "kind": "open", "mode": "read" })
    );

    assert_eq!(
        json!(EventKind::Access(AccessKind::Close(AccessMode::Write))),
        json!({ "type": "access", "kind": "close", "mode": "write" })
    );

    assert_eq!(
        json!(EventKind::Access(AccessKind::Close(AccessMode::Other))),
        json!({ "type": "access", "kind": "close", "mode": "other" })
    );

    assert_eq!(
        json!(EventKind::Access(AccessKind::Other)),
        json!({ "type": "access", "kind": "other" })
    );
}

#[cfg(feature = "serde")]
#[test]
fn create_events_are_serializable() {
    assert_eq!(
        json!(EventKind::Create(CreateKind::Any)),
        json!({ "type": "create", "kind": "any" })
    );

    assert_eq!(
        json!(EventKind::Create(CreateKind::File)),
        json!({ "type": "create", "kind": "file" })
    );

    assert_eq!(
        json!(EventKind::Create(CreateKind::Folder)),
        json!({ "type": "create", "kind": "folder" })
    );

    assert_eq!(
        json!(EventKind::Create(CreateKind::Other)),
        json!({ "type": "create", "kind": "other" })
    );
}

#[cfg(feature = "serde")]
#[test]
fn modify_events_are_serializable() {
    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Any)),
        json!({ "type": "modify", "kind": "any" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Data(DataChange::Any))),
        json!({ "type": "modify", "kind": "data", "mode": "any" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Data(DataChange::Size))),
        json!({ "type": "modify", "kind": "data", "mode": "size" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Data(DataChange::Content))),
        json!({ "type": "modify", "kind": "data", "mode": "content" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Data(DataChange::Other))),
        json!({ "type": "modify", "kind": "data", "mode": "other" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any))),
        json!({ "type": "modify", "kind": "metadata", "mode": "any" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::AccessTime
        ))),
        json!({ "type": "modify", "kind": "metadata", "mode": "access-time" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::WriteTime
        ))),
        json!({ "type": "modify", "kind": "metadata", "mode": "write-time" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Permissions
        ))),
        json!({ "type": "modify", "kind": "metadata", "mode": "permissions" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Ownership
        ))),
        json!({ "type": "modify", "kind": "metadata", "mode": "ownership" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Extended
        ))),
        json!({ "type": "modify", "kind": "metadata", "mode": "extended" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Other))),
        json!({ "type": "modify", "kind": "metadata", "mode": "other" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Name(RenameMode::Any))),
        json!({ "type": "modify", "kind": "rename", "mode": "any" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Name(RenameMode::To))),
        json!({ "type": "modify", "kind": "rename", "mode": "to" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Name(RenameMode::From))),
        json!({ "type": "modify", "kind": "rename", "mode": "from" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Name(RenameMode::Both))),
        json!({ "type": "modify", "kind": "rename", "mode": "both" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Name(RenameMode::Other))),
        json!({ "type": "modify", "kind": "rename", "mode": "other" })
    );

    assert_eq!(
        json!(EventKind::Modify(ModifyKind::Other)),
        json!({ "type": "modify", "kind": "other" })
    );
}

#[cfg(feature = "serde")]
#[test]
fn remove_events_are_serializable() {
    assert_eq!(
        json!(EventKind::Remove(RemoveKind::Any)),
        json!({ "type": "remove", "kind": "any" })
    );

    assert_eq!(
        json!(EventKind::Remove(RemoveKind::File)),
        json!({ "type": "remove", "kind": "file" })
    );

    assert_eq!(
        json!(EventKind::Remove(RemoveKind::Folder)),
        json!({ "type": "remove", "kind": "folder" })
    );

    assert_eq!(
        json!(EventKind::Remove(RemoveKind::Other)),
        json!({ "type": "remove", "kind": "other" })
    );
}
