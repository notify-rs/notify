#[macro_use] extern crate notify_backend;
extern crate notify_backend_kqueue;
extern crate tempdir;

use notify_backend_kqueue::Backend;

test_compliance!(Backend);
