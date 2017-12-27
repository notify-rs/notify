//! A standard suite of tests for `Backend` implementations.
//!
//! # Examples
//!
//! To use these tests, create a file called `tests/compliance.rs` and write:
//!
//! ```rust,ignore
//! extern crate futures;
//! #[macro_use] extern crate notify_backend;
//! extern crate notify_backend_name;
//! extern crate tempdir;
//! use notify_backend_name::Backend;
//! test_compliance!(Backend);
//! ```

/// Implements a set of compliance tests against your `Backend` implementation.
///
/// Every supported `Capability` is tested, if it can be done automatically:
///
///  - `WatchFolders`
///  - `WatchFiles`
///  - `WatchRecursively`
///
/// For internal reasons, tests covering events which your backend explicitely does not support
/// will still be run, but they will always pass.
#[macro_export]
macro_rules! test_compliance {
    ( $Backend:ident ) => (
        use futures::Async;
        use futures::stream::Stream;
        use notify_backend::prelude::*;
        use std::fs::File;
        use std::io::Write;
        use std::path::PathBuf;
        use std::thread::sleep;
        use std::time::Duration;
        use tempdir::TempDir;

        fn settle_events(backend: &mut $Backend) -> Vec<Event> {
            sleep(Duration::from_millis(25));
            let mut events: Vec<Event> = vec![];
            for _ in 0..10 {
                if let Ok(Async::Ready(Some(event))) = backend.poll() {
                    events.push(event.clone());
                }
            }

            events
        }

        #[test]
        fn cap_watch_folder() {
            if !$Backend::capabilities().contains(&Capability::WatchFolders) {
                assert!(true);
                return;
            }

            let dir = TempDir::new("cap_watch_folder").expect("create tmp dir");
            let path = dir.path().to_path_buf();
            let mut backend = $Backend::new(vec![path]).expect("init backend");

            let filepath = PathBuf::from("file.within");
            let filepathwithin = dir.path().join(&filepath);
            let mut filewithin = File::create(filepathwithin).expect("create tmp file");

            {
                let events = settle_events(&mut backend);
                assert!(events.len() > 0, "receive at least one event");

                let creates = events.iter().filter(|e| e.kind.is_create());
                assert!(creates.count() > 0, "receive at least one Create event");
            }

            writeln!(filewithin, "Everybody can talk to crickets, the trick is getting them to talk back.").expect("write to file");

            {
                let events = settle_events(&mut backend);
                assert!(events.len() > 0, "receive at least one event");

                let modifies = events.iter().filter(|e| e.kind.is_modify());
                assert!(modifies.count() > 0, "receive at least one Modify event");
            }
        }

        #[test]
        fn cap_watch_file() {
            if !$Backend::capabilities().contains(&Capability::WatchFiles) {
                assert!(true);
                return;
            }

            let dir = TempDir::new("cap_watch_file").expect("create tmp dir");
            let filepath = PathBuf::from("file.within");
            let filepathwithin = dir.path().join(&filepath);
            let mut filewithin = File::create(&filepathwithin).expect("create tmp file");

            let mut backend = $Backend::new(vec![filepathwithin]).expect("init backend");

            writeln!(filewithin, "That's a rabbit! I'm not eating a bunny rabbit.").expect("write to file");

            {
                let events = settle_events(&mut backend);
                assert!(events.len() > 0, "receive at least one event");

                let modifies = events.iter().filter(|e| e.kind.is_modify());
                assert!(modifies.count() > 0, "receive at least one Modify event");
            }
        }

        #[test]
        fn cap_watch_recursively() {
            if !$Backend::capabilities().contains(&Capability::WatchRecursively) {
                assert!(true);
                return;
            }

            let dir = TempDir::new("cap_watch_file").expect("create tmp dir");
            let path = dir.path().to_path_buf();
            let subdirpath = PathBuf::from("folder.within");
            let subdirpathwithin = dir.path().join(&subdirpath);

            let mut backend = $Backend::new(vec![path]).expect("init backend");

            let filepath = PathBuf::from("file.within");
            let filepathwithin = subdirpathwithin.join(&filepath);
            let mut filewithin = File::create(&filepathwithin).expect("create tmp file");

            {
                let events = settle_events(&mut backend);
                assert!(events.len() > 0, "receive at least one event");

                let creates = events.iter().filter(|e| e.kind.is_create());
                assert!(creates.count() > 0, "receive at least one Create event");
            }

            writeln!(filewithin, "The term is 'shipping'. And yes. Yes I am.").expect("write to file");

            {
                let events = settle_events(&mut backend);
                assert!(events.len() > 0, "receive at least one event");

                let modifies = events.iter().filter(|e| e.kind.is_modify());
                assert!(modifies.count() > 0, "receive at least one Modify event");
            }
        }
    )
}
