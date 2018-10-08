//extern crate notify;
extern crate notify_backend as backend;

use backend::event;

use std::mem::size_of;
use std::path::PathBuf;

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

 - paths: Vec<PathBuf>:  {}
 - attrs: AnyMap:        {}
 - kind:  EventKind:     {}
",
        size_of::<event::Event>(),
        size_of::<Vec<PathBuf>>(),
        size_of::<event::AnyMap>(),
        size_of::<event::EventKind>(),
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
    );
}
