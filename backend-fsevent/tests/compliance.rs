extern crate futures;
#[macro_use] extern crate notify_backend;
extern crate notify_backend_fsevent;
extern crate tempdir;

use notify_backend_fsevent::Backend;

test_compliance!(Backend);
