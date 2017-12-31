extern crate notify_backend as backend;

extern crate notify_backend_poll_tree as poll_tree;

#[cfg(any(
    target_os = "linux",
    target_os = "android",
))]
extern crate notify_backend_inotify as inotify;

#[cfg(any(
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
extern crate notify_backend_kqueue as kqueue;

use backend::prelude::*;
use std::path::PathBuf;

fn new_backend(paths: Vec<PathBuf>, use_polling: bool) -> BackendResult<BoxedBackend> {
    let mut result = Err(BackendError::Generic("you should never see this".into()));

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
    ))]
    { result = new_backend_if_bad(result, &paths, |paths| inotify::Backend::new(paths)); }

    #[cfg(any(
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    { result = new_backend_if_bad(result, &paths, |paths| kqueue::Backend::new(paths)); }

    if use_polling {
        result = new_backend_if_bad(result, &paths, |paths| poll_tree::Backend::new(paths));
    }

    result
}

fn new_backend_if_bad<F>(
    previous: BackendResult<BoxedBackend>,
    paths: &Vec<PathBuf>,
    backfn: F
) -> BackendResult<BoxedBackend>
where F: FnOnce(Vec<PathBuf>) -> BackendResult<BoxedBackend>
{
    match previous {
        p @ Ok(_) => p,
        Err(err) => match err {
            e @ BackendError::NonExistent(_) => Err(e),
            _ => backfn(paths.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}
