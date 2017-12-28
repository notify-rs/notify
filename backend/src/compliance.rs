//! The standard suite of tests for `Backend` implementations.
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
//!
//! use notify_backend_name::Backend;
//!
//! test_compliance!(Backend);
//! ```

/// Implements a set of compliance tests against your `Backend` implementation.
///
/// Every supported `Capability` is tested, if it can be done automatically:
///
///  - `WatchFolders`
///  - `WatchFiles`
///  - `WatchRecursively`
///  - `EmitOnAccess`
///  - `FollowSymlinks`
///  - `TrackRelated`
///
/// For internal reasons, tests covering events which your backend explicitely does not support
/// will still be run, but they will always pass.
#[macro_export]
macro_rules! test_compliance {
    ( $Backend:ident ) => (
        use futures::Async;
        use futures::stream::Stream;
        use notify_backend::prelude::*;
        use std::fs::{File, rename};
        use std::io::Write;
        use std::path::PathBuf;
        use std::thread::sleep;
        use std::time::Duration;
        use tempdir::TempDir;
        #[cfg(unix)]
        use std::os::unix::fs::symlink;

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

        #[test]
        fn cap_emit_on_access() {
            if !$Backend::capabilities().contains(&Capability::EmitOnAccess) {
                assert!(true);
                return;
            }

            if $Backend::capabilities().contains(&Capability::WatchFiles) {
                let dir = TempDir::new("cap_emit_on_access").expect("create tmp dir");
                let filename = String::from("file");
                let filepath = dir.path().join(&filename);
                File::create(&filepath).expect("create tmp file");

                let mut backend = $Backend::new(vec![filepath.clone()]).expect("init backend");

                File::open(&filepath).expect("open tmp file");

                {
                    let events = settle_events(&mut backend);
                    assert!(events.len() > 0, "receive at least one event");

                    let accesses = events.iter().filter(|e| e.kind.is_access());
                    assert!(accesses.count() > 0, "receive at least one Access event");
                }
            } else {
                unimplemented!();
            }
        }

        #[test]
        fn cap_follow_symlinks() {
            if !$Backend::capabilities().contains(&Capability::FollowSymlinks) {
                assert!(true);
                return;
            }

            if $Backend::capabilities().contains(&Capability::WatchFiles) {
                let dir = TempDir::new("cap_emit_on_access").expect("create tmp dir");
                let filename = String::from("file");
                let filepath = dir.path().join(&filename);
                let mut file = File::create(&filepath).expect("create tmp file");
                let linkname = String::from("link");
                let linkpath = dir.path().join(&linkname);
                if cfg!(unix) {
                    symlink(&filepath, &linkpath).expect("create symlink");
                } else {
                    unimplemented!();
                }

                let mut backend = $Backend::new(vec![linkpath]).expect("init backend");

                writeln!(file, "Everybody can talk to crickets, the trick is getting them to talk back.").expect("write to file");

                {
                    let events = settle_events(&mut backend);
                    assert!(events.len() > 0, "receive at least one event");

                    let modifies = events.iter().filter(|e| e.kind.is_modify());
                    assert!(modifies.count() > 0, "receive at least one Modify event");
                }
            } else {
                unimplemented!();
            }
        }

        #[test]
        fn cap_track_related() {
            if !$Backend::capabilities().contains(&Capability::TrackRelated) {
                assert!(true);
                return;
            }

            if $Backend::capabilities().contains(&Capability::WatchFolders) {
                let dir = TempDir::new("cap_emit_on_access").expect("create tmp dir");
                let path = dir.path().to_path_buf();
                let filename_a = String::from("file_a");
                let filepath_a = dir.path().join(&filename_a);
                let filename_b = String::from("file_b");
                let filepath_b = dir.path().join(&filename_b);
                File::create(&filepath_a).expect("create tmp file");

                let mut backend = $Backend::new(vec![path]).expect("init backend");

                rename(&filepath_a, &filepath_b).expect("rename file");

                {
                    let events = settle_events(&mut backend);
                    assert!(events.len() > 0, "receive at least one event");

                    let modify_events_with_relids = events.iter().filter(|e| e.kind.is_modify() && e.relid.is_some()).collect::<Vec<_>>();

                    if modify_events_with_relids.len() > 0 {
                        let relid = modify_events_with_relids[0].relid;
                        let modifies = modify_events_with_relids.iter().filter(|e| e.relid == relid);
                        assert!(modifies.count() == 2, "receive exactly two related Modify events");
                    } else {
                        let modifies = events.iter().filter(|e| e.kind.is_modify() && e.paths.len() > 1);
                        assert!(modifies.count() > 0, "receive related Modify events");
                    }
                }
            } else {
                unimplemented!();
            }
        }
    )
}
