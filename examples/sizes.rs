//extern crate notify;
extern crate notify_backend as backend;

use backend::event;

use std::mem::size_of;
use std::path::PathBuf;

struct EventSlim {
    pub kind: event::EventKind,
    pub attrs: event::AnyMap,
}

fn main() {
    println!(
        "
Sizes of types
=============="
    );

    println!(
        "
## Event: {}

Struct. Sum of field sizes.

 - kind:  EventKind:       {}
 - path:  Option<PathBuf>: {}
 - attrs: AnyMap:          {}

 - PathBuf:                {}
",
        size_of::<event::Event>(),
        size_of::<event::EventKind>(),
        size_of::<Option<PathBuf>>(),
        size_of::<event::AnyMap>(),
        size_of::<PathBuf>(),
    );

    println!(
        "
## Event (slim): {}

Struct. Sum of field sizes.

 - kind:  EventKind:       {}
 - attrs: AnyMap:          {}
",
        size_of::<EventSlim>(),
        size_of::<event::EventKind>(),
        size_of::<event::AnyMap>(),
    );

    println!(
        "
### EventKind: {}

Enum. Size of largest variant (*) + 1-8 bytes.

 - AccessKind: {}
   + AccessMode: {}
 - CreateKind: {}
 - ModifyKind: {}
   + DataChange: {}
   + MetadataKind: {}
   + RenameMode: {}
 - RemoveKind: {}
 - Option<NonZeroU16>: {}
",
        size_of::<event::EventKind>(),
        size_of::<event::AccessKind>(),
        size_of::<event::AccessMode>(),
        size_of::<event::CreateKind>(),
        size_of::<event::ModifyKind>(),
        size_of::<event::DataChange>(),
        size_of::<event::MetadataKind>(),
        size_of::<event::RenameMode>(),
        size_of::<event::RemoveKind>(),
        size_of::<Option<std::num::NonZeroU16>>(),
    );
}
