use backend::prelude::*;

pub fn normalise(backend: BoxedBackend) -> BoxedBackend {
    let caps = backend.caps();

    if !caps.contains(&Capability::WatchRecursively) {
        //
    }

    backend
}

