extern crate futures;
#[macro_use] extern crate notify_backend;
extern crate notify_backend_fsevent;
extern crate tempdir;

use notify_backend_fsevent::Backend;

use futures::Async;
use futures::stream::Stream;
use notify_backend::prelude::*;
use std::fs::{File, OpenOptions, rename, create_dir};
use std::io::Write;
use std::path::PathBuf;
use std::thread::{sleep, self};
use std::time::Duration;
use tempdir::TempDir;
#[cfg(unix)]
use std::os::unix::fs::symlink;

fn settle_events(backend: &mut BoxedBackend) -> Vec<Event> {
    sleep(Duration::from_millis(500));
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
    if !Backend::capabilities().contains(&Capability::WatchFolders) {
        return assert!(true);
    }

    let dir = TempDir::new("cap_watch_folder").expect("create tmp dir");
    let path = dir.path().to_path_buf();
    let mut backend = Backend::new(vec![path]).expect("init backend");

    let filepath = PathBuf::from("file.within");
    let filepathwithin = dir.path().join(&filepath);
    {
        let mut filewithin = File::create(&filepathwithin)
            .expect("create tmp file");
    }

    thread::sleep(Duration::from_millis(1000));

    {
        let events = settle_events(&mut backend);
        assert!(events.len() > 0, "receive at least one event");

        let creates = events.iter().filter(|e| e.kind.is_create());
        assert!(creates.count() > 0, "receive at least one Create event");
    }
    {
        let mut filewithin = File::create(&filepathwithin).expect("create tmp file");
        writeln!(filewithin, "Everybody can talk to crickets, the trick is \
        getting them to talk back.")
            .expect("write to file");
    }
        {
            let events = settle_events(&mut backend);
            assert!(events.len() > 0, "receive at least one mod event");

            let modifies = events.iter().filter(|e| e.kind.is_modify());
            assert!(modifies.count() > 0, "receive at least one Modify event");
        }
}

#[test]
fn cap_watch_file() {
    if !Backend::capabilities().contains(&Capability::WatchFiles) {
        return assert!(true);
    }

    let dir = TempDir::new("cap_watch_file").expect("create tmp dir");
    let filepath = PathBuf::from("file.within");
    let filepathwithin = dir.path().join(&filepath);
    {
        let mut filewithin = File::create(&filepathwithin)
            .expect("create tmp file");
    }

    let mut backend = Backend::new(vec![filepathwithin.clone()])
        .expect("init backend");

    {
        let mut f = OpenOptions::new().append(true).open(&filepathwithin)
            .unwrap();
        writeln!(&f, "That's a rabbit! I'm not eating a bunny rabbit.")
            .expect("write to file");
    }

    thread::sleep(Duration::from_millis(1000));

    {
        let events = settle_events(&mut backend);
        assert!(events.len() > 0, "receive at least one event");
        eprintln!("events {:?}", &events);

        let modifies = events.iter().filter(|e| e.kind.is_modify());
        assert!(modifies.count() > 0, "receive at least one Modify event");
    }
}

#[test]
fn cap_watch_recursively() {
    if !Backend::capabilities().contains(&Capability::WatchRecursively) {
        return assert!(true);
    }

    let dir = TempDir::new("cap_watch_recursively").expect("create tmp dir");
    let path = dir.path().to_path_buf();
    let subdirpath = PathBuf::from("folder.within");
    let subdirpathwithin = dir.path().join(&subdirpath);
    create_dir(&subdirpathwithin);

    eprintln!("watching {:?}", &path);
    let mut backend = Backend::new(vec![path]).expect("init backend");
    thread::sleep(Duration::from_millis(1000));

    let filepath = PathBuf::from("file.within");
    let filepathwithin = subdirpathwithin.join(&filepath);
    {
        let mut filewithin = File::create(&filepathwithin)
            .expect("create tmp file");
    }
    thread::sleep(Duration::from_millis(1000));

    {
        let events = settle_events(&mut backend);
        assert!(events.len() > 0, "receive at least one event");

        let creates = events.iter().filter(|e| e.kind.is_create());
        assert!(creates.count() > 0, "receive at least one Create event");
    }

    {
        let mut f = OpenOptions::new().append(true).open(&filepathwithin)
            .unwrap();
        writeln!(f, "The term is 'shipping'. And yes. Yes I am.")
        .expect("write to file");
    }

    thread::sleep(Duration::from_millis(1000));

    {
        let events = settle_events(&mut backend);
        assert!(events.len() > 0, "receive at least one event");

        let modifies = events.iter().filter(|e| e.kind.is_modify());
        assert!(modifies.count() > 0, "receive at least one Modify event");
    }
}

#[test]
fn cap_emit_on_access() {
    if !Backend::capabilities().contains(&Capability::EmitOnAccess) {
        return assert!(true);
    }

    if Backend::capabilities().contains(&Capability::WatchFiles) {
        let dir = TempDir::new("cap_emit_on_access").expect("create tmp dir");
        let filename = String::from("file");
        let filepath = dir.path().join(&filename);
        File::create(&filepath).expect("create tmp file");

        let mut backend = Backend::new(vec![filepath.clone()]).expect("init backend");

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
    if !Backend::capabilities().contains(&Capability::FollowSymlinks) {
        return assert!(true);
    }

    if Backend::capabilities().contains(&Capability::WatchFiles) {
        let dir = TempDir::new("follow_symlink").expect("create tmp dir");
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

        let mut backend = Backend::new(vec![linkpath]).expect("init backend");

        {
            writeln!(file, "Everybody can talk to crickets, the trick is \
            getting them to talk back.")
                .expect("write to file");
        }

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
    if !Backend::capabilities().contains(&Capability::TrackRelated) {
        return assert!(true);
    }

    if Backend::capabilities().contains(&Capability::WatchFolders) {
        let dir = TempDir::new("track_related").expect("create tmp dir");
        let path = dir.path().to_path_buf();
        let filename_a = String::from("file_a");
        let filepath_a = dir.path().join(&filename_a);
        let filename_b = String::from("file_b");
        let filepath_b = dir.path().join(&filename_b);
        File::create(&filepath_a).expect("create tmp file");

        let mut backend = Backend::new(vec![path]).expect("init backend");
        thread::sleep(Duration::from_millis(500));

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
