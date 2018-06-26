#[macro_use]
extern crate notify_backend;
extern crate notify_backend_poll_tree;
extern crate tempdir;

use notify_backend_poll_tree::Backend;

test_compliance!(Backend);
