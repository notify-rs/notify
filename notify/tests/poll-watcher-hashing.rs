// The mtime-vs-content event-kind distinction this test asserts is validated against Linux
// filesystem semantics. On macOS (APFS) a same-content rewrite with a fresh mtime is reported as
// `Modify(Data)` rather than `Modify(Metadata(WriteTime))`, so the test is Linux-only.
#![cfg(target_os = "linux")]
use nix::sys::stat::futimens;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime};
use std::{fs, sync, thread};

use nix::sys::time::TimeSpec;
use tempfile::TempDir;

use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind};
use notify::{Config, Event, EventKind, PollWatcher, RecursiveMode, Watcher};

#[test]
fn test_poll_watcher_distinguish_modify_kind() {
    let mut harness = TestHarness::setup();
    harness.watch_tempdir();

    let testfile = harness.create_file("testfile");
    harness.expect_recv(&testfile, EventKind::Create(CreateKind::Any));
    harness.advance_clock();

    harness.write_file(&testfile, "data1");
    harness.expect_recv(
        &testfile,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)),
    );
    harness.advance_clock();

    harness.write_file_keep_time(&testfile, "data2");
    harness.expect_recv(
        &testfile,
        EventKind::Modify(ModifyKind::Data(DataChange::Any)),
    );
    harness.advance_clock();

    harness.write_file(&testfile, "data2");
    harness.expect_recv(
        &testfile,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)),
    );
}

struct TestHarness {
    testdir: TempDir,
    watcher: PollWatcher,
    rx: Receiver<notify::Result<Event>>,
}

impl TestHarness {
    pub fn setup() -> Self {
        let tempdir = tempfile::tempdir().unwrap();

        let config = Config::default()
            .with_compare_contents(true)
            .with_poll_interval(Duration::from_millis(10));
        let (tx, rx) = sync::mpsc::channel();
        let watcher = PollWatcher::new(
            move |event: notify::Result<Event>| {
                tx.send(event).unwrap();
            },
            config,
        )
        .unwrap();

        Self {
            testdir: tempdir,
            watcher,
            rx,
        }
    }

    pub fn watch_tempdir(&mut self) {
        self.watcher
            .watch(self.testdir.path(), RecursiveMode::Recursive)
            .unwrap();
    }

    pub fn create_file(&self, name: &str) -> PathBuf {
        let path = self.testdir.path().join(name);
        fs::File::create(&path).unwrap();
        path
    }

    pub fn write_file<P: AsRef<Path>>(&self, path: P, contents: &str) {
        self.write_file_common(path.as_ref(), contents);
    }

    pub fn write_file_keep_time<P: AsRef<Path>>(&self, path: P, contents: &str) {
        let metadata = fs::metadata(path.as_ref()).unwrap();
        let file = self.write_file_common(path.as_ref(), contents);
        let atime = Self::to_timespec(metadata.accessed().unwrap());
        let mtime = Self::to_timespec(metadata.modified().unwrap());
        futimens(&file, &atime, &mtime).unwrap();
    }

    fn write_file_common(&self, path: &Path, contents: &str) -> File {
        let mut file = OpenOptions::new().write(true).open(path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        file
    }

    fn to_timespec(t: SystemTime) -> TimeSpec {
        TimeSpec::from_duration(t.duration_since(SystemTime::UNIX_EPOCH).unwrap())
    }

    pub fn advance_clock(&self) {
        // Unfortunately this entire crate is pretty dependent on real syscall behaviour so let's
        // test "for real" and require a sleep long enough to trigger mtime actually increasing.
        thread::sleep(Duration::from_secs(1));
    }

    fn expect_recv<P: AsRef<Path>>(&self, expected_path: P, expected_kind: EventKind) {
        let actual = self
            .rx
            .recv_timeout(Duration::from_secs(15))
            .unwrap()
            .expect("Watch I/O error not expected under test");
        assert_eq!(actual.paths, vec![expected_path.as_ref().to_path_buf()]);
        assert_eq!(expected_kind, actual.kind);
    }
}
