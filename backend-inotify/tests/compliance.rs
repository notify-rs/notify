extern crate futures;
#[macro_use] extern crate notify_backend;
extern crate notify_backend_inotify;
extern crate tempdir;

use notify_backend_inotify::Backend;

test_compliance!(Backend);
