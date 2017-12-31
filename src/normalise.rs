use backend::prelude::*;

pub fn normalise(backend: BoxedBackend) -> BoxedBackend {
    let caps = backend.capabilities();

    if !caps.contains(&Capability::WatchRecursively) {
        //
    }

    backend
}

