#![allow(dead_code)] // because its are test helpers

use std::{
    fmt::Debug,
    ops::Deref,
    path::{Path, PathBuf},
    sync::mpsc::{self, TryRecvError},
    time::Duration,
};

use notify_types::event::{Event, EventKind};

use crate::{Config, Error, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher, WatcherKind};
use pretty_assertions::assert_eq;

pub struct Receiver {
    pub rx: mpsc::Receiver<Result<Event, Error>>,
    pub timeout: Duration,
    pub detect_changes: Option<Box<dyn Fn()>>,
    pub kind: WatcherKind,
}

impl Receiver {
    const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);

    /// Waits for events in the same order as they are provided,
    /// and fails if it encounters an unexpected one.
    ///
    /// Before any actions, it'll call [`Self::detect_changes`].
    /// It simplify any tests with [`PollWatcher`]
    pub fn wait_exact(&mut self, expected: impl IntoIterator<Item = ExpectedEvent>) {
        self.detect_changes();
        for (idx, expected) in expected.into_iter().enumerate() {
            match self.try_recv() {
                Ok(result) => match result {
                    Ok(event) => assert_eq!(
                        ExpectedEvent::from_event(event),
                        expected,
                        "Unexpected event by index {idx}"
                    ),
                    Err(err) => panic!("Expected an event by index {idx} but got {err:?}"),
                },
                Err(err) => panic!("Unable to check the event by index {idx}: {err:?}"),
            }
        }
        match self.rx.try_recv() {
            Ok(res) => panic!("Unexpected extra event: {res:?}"),
            Err(err) => assert!(
                matches!(err, TryRecvError::Empty),
                "Unexpected error: expected Empty, actual: {err:?}"
            ),
        }
    }

    /// Waits for the provided events in any order and ignores unexpected ones.
    ///
    /// Before any actions, it'll call [`Self::detect_changes`].
    /// It simplify any tests with [`PollWatcher`]
    pub fn wait(&mut self, iter: impl IntoIterator<Item = ExpectedEvent>) {
        self.detect_changes();
        let mut expected = iter.into_iter().enumerate().collect::<Vec<_>>();
        let mut received = Vec::new();

        while !expected.is_empty() {
            match self.try_recv() {
                Ok(result) => match result {
                    Ok(event) => {
                        received.push(event.clone());
                        let mut found_idx = None;
                        let actual = ExpectedEvent::from_event(event);
                        for (idx, (_, expected)) in expected.iter().enumerate() {
                            if &actual == expected {
                                found_idx = Some(idx);
                                break;
                            }
                        }
                        if let Some(found_idx) = found_idx {
                            expected.swap_remove(found_idx);
                        }
                    }
                    Err(err) => panic!("Got an error from the watcher {:?}: {err:?}. Received: {received:#?}. Weren't received: {expected:#?}", self.kind),
                },
                Err(err) => panic!("Watcher {:?} recv error: {err:?}. Received: {received:#?}. Weren't received: {expected:#?}", self.kind),
            }
        }
    }

    /// Waits a specific event.
    ///
    /// Panics, if got an error
    pub fn wait_event(&mut self, filter: impl Fn(&Event) -> bool) -> Event {
        self.detect_changes();
        let mut received = Vec::new();
        loop {
            match self.try_recv() {
                Ok(res) => match res {
                    Ok(event) => {
                        if filter(&event) {
                            return event;
                        }
                        received.push(event);
                    },
                    Err(err) => panic!("Got an error from the watcher {:?} before the expected event has been received: {err:?}. Received: {received:#?}", self.kind),
                },
                Err(err) => panic!("Watcher {:?} recv error but expected event weren't received: {err:?}. Received: {received:#?}", self.kind),
            }
        }
    }

    pub fn try_recv(&mut self) -> Result<Result<Event, Error>, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(self.timeout)
    }

    pub fn recv(&mut self) -> Event {
        self.recv_result()
            .unwrap_or_else(|e| panic!("Unexpected error from the watcher {:?}: {e:?}", self.kind))
    }

    pub fn recv_result(&mut self) -> Result<Event, Error> {
        self.try_recv().unwrap_or_else(|e| match e {
            mpsc::RecvTimeoutError::Timeout => panic!("Unable to wait the next event from the watcher {:?}: timeout", self.kind),
            mpsc::RecvTimeoutError::Disconnected => {
                panic!("Unable to wait the next event: the watcher {:?} part of the channel was disconnected", self.kind)
            }
        })
    }

    pub fn detect_changes(&self) {
        if let Some(detect_changes) = self.detect_changes.as_deref() {
            detect_changes()
        }
    }

    /// Returns an iterator iterating by events
    ///
    /// It doesn't fail on timeout, instead it returns None
    ///
    /// This behaviour is better for tests, because allows us to determine which events was received
    pub fn iter(&mut self) -> impl Iterator<Item = Event> + '_ {
        struct Iter<'a> {
            rx: &'a mut Receiver,
        }

        impl Iterator for Iter<'_> {
            type Item = Event;

            fn next(&mut self) -> Option<Self::Item> {
                self.rx
                    .try_recv()
                    .ok()
                    .map(|res| res.unwrap_or_else(|err| panic!("Got an error: {err:#?}")))
            }
        }

        Iter { rx: self }
    }

    /// Ensures, that the receiver part is empty. It doesn't wait anything, just check the channel
    pub fn ensure_empty(&mut self) {
        if let Ok(event) = self.rx.try_recv() {
            panic!("Unexpected event was received: {event:#?}")
        }
    }
}

#[derive(Debug)]
pub struct ChannelConfig {
    timeout: Duration,
    watcher_config: Config,
}

impl ChannelConfig {
    pub fn with_watcher_config(mut self, watcher_config: Config) -> Self {
        self.watcher_config = watcher_config;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            timeout: Receiver::DEFAULT_TIMEOUT,
            watcher_config: Default::default(),
        }
    }
}

pub struct TestWatcher<W> {
    pub watcher: W,
    pub kind: WatcherKind,
}

impl<W: Watcher> TestWatcher<W> {
    pub fn watch_recursively(&mut self, path: impl AsRef<Path>) {
        self.watch(path, RecursiveMode::Recursive);
    }

    pub fn watch_nonrecursively(&mut self, path: impl AsRef<Path>) {
        self.watch(path, RecursiveMode::NonRecursive);
    }

    pub fn watch(&mut self, path: impl AsRef<Path>, recursive_mode: RecursiveMode) {
        let path = path.as_ref();
        self.watcher
            .watch(path, recursive_mode)
            .unwrap_or_else(|e| panic!("Unable to watch {:?}: {e:#?}", path))
    }
}

pub fn channel_with_config<W: Watcher>(config: ChannelConfig) -> (TestWatcher<W>, Receiver) {
    let (tx, rx) = mpsc::channel();
    let watcher = W::new(tx, config.watcher_config).expect("Unable to create a watcher");
    (
        TestWatcher {
            watcher,
            kind: W::kind(),
        },
        Receiver {
            rx,
            timeout: config.timeout,
            detect_changes: None,
            kind: W::kind(),
        },
    )
}

pub fn channel<W: Watcher>() -> (TestWatcher<W>, Receiver) {
    channel_with_config(Default::default())
}

pub fn recommended_channel() -> (TestWatcher<RecommendedWatcher>, Receiver) {
    channel()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedEvent {
    kind: EventKind,
    paths: Vec<PathBuf>,
}

impl ExpectedEvent {
    pub fn new(kind: EventKind, paths: Vec<PathBuf>) -> Self {
        Self { kind, paths }
    }

    pub fn with_path(kind: EventKind, path: impl AsRef<Path>) -> Self {
        Self::new(kind, vec![path.as_ref().to_path_buf()])
    }

    pub fn from_event(e: Event) -> Self {
        Self {
            kind: e.kind,
            paths: e.paths,
        }
    }
}

pub fn testdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("Unable to create tempdir")
}

/// Creates a [`PollWatcher`] with comparable content and manual polling.
///
/// Returned [`Receiver`] will send a message to poll changes before wait-methods
pub fn poll_watcher() -> (TestWatcher<PollWatcher>, Receiver) {
    let (tx, rx) = mpsc::channel();
    let watcher = PollWatcher::new(
        tx,
        Config::default()
            .with_compare_contents(true)
            .with_manual_polling(),
    )
    .expect("Unable to create PollWatcher");
    let sender = watcher.poll_sender();
    let watcher = TestWatcher {
        watcher,
        kind: PollWatcher::kind(),
    };
    let rx = Receiver {
        rx,
        timeout: Receiver::DEFAULT_TIMEOUT,
        detect_changes: Some(Box::new(move || {
            sender
                .send(())
                .expect("PollWatcher receiver part was disconnected")
        })),
        kind: watcher.kind,
    };

    (watcher, rx)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cookies(Vec<usize>);

impl Cookies {
    /// Pushes new cookie if it is some and is not equal to the last
    pub fn try_push(&mut self, event: &Event) -> bool {
        let Some(tracker) = event.attrs.tracker() else {
            return false;
        };

        if self.0.last() != Some(&tracker) {
            self.0.push(tracker);
            true
        } else {
            false
        }
    }

    pub fn ensure_len<E: Debug>(&self, len: usize, events: &[E]) {
        assert_eq!(
            self.len(),
            len,
            "Unexpected cookies len. events: {events:#?}"
        )
    }
}

impl Deref for Cookies {
    type Target = [usize];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct StoreCookies<'a, I> {
    cookies: &'a mut Cookies,
    inner: I,
}

impl<I: Iterator<Item = Event>> Iterator for StoreCookies<'_, I> {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        let event = self.inner.next()?;
        self.cookies.try_push(&event);
        Some(event)
    }
}

pub struct IgnoreAccess<I> {
    inner: I,
}

impl<I: Iterator<Item = Event>> Iterator for IgnoreAccess<I> {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let event = self.inner.next()?;

            if !event.kind.is_access() {
                break Some(event);
            }
        }
    }
}

pub trait TestIteratorExt: Sized {
    /// Skips any [`EventKind::Access`] events
    fn ignore_access(self) -> IgnoreAccess<Self>
    where
        Self: Sized,
    {
        IgnoreAccess { inner: self }
    }

    /// Stores any encountered [`notify_types::event::EventAttributes::tracker`] into the provided [`Cookies`]
    fn store_cookies(self, cookies: &mut Cookies) -> StoreCookies<'_, Self> {
        StoreCookies {
            cookies,
            inner: self,
        }
    }
}

impl<I: Iterator<Item = Event>> TestIteratorExt for I {}

pub use actual_ctors::*;
pub use expected_ctors::*;

#[rustfmt::skip] // due to annoying macro invocations formatting 
mod actual_ctors {
    use notify_types::event::{
        AccessKind, AccessMode, CreateKind, DataChange, Event, EventKind, MetadataKind, ModifyKind,
        RemoveKind, RenameMode,
    };

    use super::ExpectedEvent;

    pub fn actual(event: Event) -> ExpectedEvent {
        ExpectedEvent::from_event(event)
    }

    macro_rules! actual_if_matches {
        ($name: ident, $pattern:pat $(if $guard:expr)?) => {
            pub fn $name(event: Event) -> Option<ExpectedEvent> {
                actual_if(event, |kind| matches!(kind, $pattern $(if $guard)?))
            }
        };
    }
    
    fn actual_if(event: Event, f: impl FnOnce(EventKind) -> bool) -> Option<ExpectedEvent> {
        if f(event.kind) {
            Some(actual(event))
        } else {
            None
        }
    }

    actual_if_matches!(actual_any, EventKind::Any);
    actual_if_matches!(actual_other, EventKind::Other);
    actual_if_matches!(actual_create, EventKind::Create(_));
    actual_if_matches!(actual_create_any, EventKind::Create(CreateKind::Any));
    actual_if_matches!(actual_create_other, EventKind::Create(CreateKind::Other));
    actual_if_matches!(actual_create_file, EventKind::Create(CreateKind::File));
    actual_if_matches!(actual_create_folder, EventKind::Create(CreateKind::Folder));

    actual_if_matches!(actual_remove, EventKind::Remove(_));
    actual_if_matches!(actual_remove_any, EventKind::Remove(RemoveKind::Any));
    actual_if_matches!(actual_remove_other, EventKind::Remove(RemoveKind::Other));
    actual_if_matches!(actual_remove_file, EventKind::Remove(RemoveKind::File));
    actual_if_matches!(actual_remove_folder, EventKind::Remove(RemoveKind::Folder));

    actual_if_matches!(actual_modify, EventKind::Modify(_));
    actual_if_matches!(actual_modify_any, EventKind::Modify(ModifyKind::Any));
    actual_if_matches!(actual_modify_other, EventKind::Modify(ModifyKind::Other));

    actual_if_matches!(actual_modify_data, EventKind::Modify(ModifyKind::Data(_)));
    actual_if_matches!(actual_modify_data_any, EventKind::Modify(ModifyKind::Data(DataChange::Any)));
    actual_if_matches!(actual_modify_data_other, EventKind::Modify(ModifyKind::Data(DataChange::Other)));
    actual_if_matches!(actual_modify_data_content, EventKind::Modify(ModifyKind::Data(DataChange::Content)));
    actual_if_matches!(actual_modify_data_size, EventKind::Modify(ModifyKind::Data(DataChange::Size)));

    actual_if_matches!(actual_modify_meta,EventKind::Modify(ModifyKind::Metadata(_)));
    actual_if_matches!(actual_modify_meta_any,EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)));
    actual_if_matches!(actual_modify_meta_other, EventKind::Modify(ModifyKind::Metadata(MetadataKind::Other)));
    actual_if_matches!(actual_modify_meta_extended, EventKind::Modify(ModifyKind::Metadata(MetadataKind::Extended)));
    actual_if_matches!(actual_modify_meta_owner, EventKind::Modify(ModifyKind::Metadata(MetadataKind::Ownership)));
    actual_if_matches!(actual_modify_meta_perm, EventKind::Modify(ModifyKind::Metadata(MetadataKind::Permissions)));
    actual_if_matches!(actual_modify_meta_mtime, EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)));
    actual_if_matches!(actual_modify_meta_atime, EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)));

    actual_if_matches!(actual_rename, EventKind::Modify(ModifyKind::Name(_)));
    actual_if_matches!(actual_rename_from, EventKind::Modify(ModifyKind::Name(RenameMode::From)));
    actual_if_matches!(actual_rename_to, EventKind::Modify(ModifyKind::Name(RenameMode::To)));
    actual_if_matches!(actual_rename_both, EventKind::Modify(ModifyKind::Name(RenameMode::Both)));

    actual_if_matches!(actual_access, EventKind::Access(_));
    actual_if_matches!(actual_access_any, EventKind::Access(AccessKind::Any));
    actual_if_matches!(actual_access_other, EventKind::Access(AccessKind::Other));
    actual_if_matches!(actual_access_read, EventKind::Access(AccessKind::Read));
    actual_if_matches!(actual_access_open, EventKind::Access(AccessKind::Open(_)));
    actual_if_matches!(actual_access_open_any, EventKind::Access(AccessKind::Open(AccessMode::Any)));
    actual_if_matches!(actual_access_open_other, EventKind::Access(AccessKind::Open(AccessMode::Other)));
    actual_if_matches!(actual_access_open_read, EventKind::Access(AccessKind::Open(AccessMode::Read)));
    actual_if_matches!(actual_access_open_write, EventKind::Access(AccessKind::Open(AccessMode::Write)));
    actual_if_matches!(actual_access_open_exec, EventKind::Access(AccessKind::Open(AccessMode::Execute)));
    actual_if_matches!(actual_access_close, EventKind::Access(AccessKind::Close(_)));
    actual_if_matches!(actual_access_close_any, EventKind::Access(AccessKind::Close(AccessMode::Any)));
    actual_if_matches!(actual_access_close_other, EventKind::Access(AccessKind::Close(AccessMode::Other)));
    actual_if_matches!(actual_access_close_read, EventKind::Access(AccessKind::Close(AccessMode::Read)));
    actual_if_matches!(actual_access_close_write, EventKind::Access(AccessKind::Close(AccessMode::Write)));
    actual_if_matches!(actual_access_close_exec, EventKind::Access(AccessKind::Close(AccessMode::Execute)));
}

mod expected_ctors {
    use std::path::Path;

    use notify_types::event::{
        AccessKind, AccessMode, CreateKind, DataChange, EventKind, MetadataKind, ModifyKind,
        RemoveKind, RenameMode,
    };

    use crate::test::ExpectedEvent;

    pub fn expected(kind: EventKind, path: impl AsRef<Path>) -> ExpectedEvent {
        ExpectedEvent::with_path(kind, path)
    }

    pub fn expected_any(path: impl AsRef<Path>) -> ExpectedEvent {
        ExpectedEvent::with_path(EventKind::Any, path)
    }

    pub fn expected_other(path: impl AsRef<Path>) -> ExpectedEvent {
        ExpectedEvent::with_path(EventKind::Other, path)
    }

    pub fn expected_create(kind: CreateKind, path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Create(kind), path)
    }

    pub fn expected_create_any(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_create(CreateKind::Any, path)
    }

    pub fn expected_create_other(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_create(CreateKind::Other, path)
    }

    pub fn expected_create_file(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_create(CreateKind::File, path)
    }

    pub fn expected_create_folder(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_create(CreateKind::Folder, path)
    }

    pub fn expected_remove(kind: RemoveKind, path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Remove(kind), path)
    }

    pub fn expected_remove_any(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Remove(RemoveKind::Any), path)
    }

    pub fn expected_remove_other(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Remove(RemoveKind::Other), path)
    }

    pub fn expected_remove_file(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Remove(RemoveKind::File), path)
    }

    pub fn expected_remove_folder(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Remove(RemoveKind::Folder), path)
    }

    pub fn expected_modify(kind: ModifyKind, path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Modify(kind), path)
    }

    pub fn expected_modify_any(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Modify(ModifyKind::Any), path)
    }

    pub fn expected_modify_other(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Modify(ModifyKind::Other), path)
    }

    pub fn expected_modify_data(change: DataChange, path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Modify(ModifyKind::Data(change)), path)
    }

    pub fn expected_modify_data_any(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Modify(ModifyKind::Data(DataChange::Any)), path)
    }

    pub fn expected_modify_data_other(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Modify(ModifyKind::Data(DataChange::Other)), path)
    }

    pub fn expected_modify_data_content(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            path,
        )
    }

    pub fn expected_modify_data_size(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Modify(ModifyKind::Data(DataChange::Size)), path)
    }

    pub fn expected_modify_meta(kind: MetadataKind, path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Modify(ModifyKind::Metadata(kind)), path)
    }

    pub fn expected_modify_meta_any(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
            path,
        )
    }

    pub fn expected_modify_meta_other(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Other)),
            path,
        )
    }

    pub fn expected_modify_meta_atime(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)),
            path,
        )
    }

    pub fn expected_modify_meta_mtime(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)),
            path,
        )
    }

    pub fn expected_modify_meta_extended(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Extended)),
            path,
        )
    }

    pub fn expected_modify_meta_owner(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Ownership)),
            path,
        )
    }

    pub fn expected_modify_meta_perm(path: impl AsRef<Path>) -> ExpectedEvent {
        expected(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Permissions)),
            path,
        )
    }

    pub fn expected_rename(mode: RenameMode, path: impl AsRef<Path>) -> ExpectedEvent {
        expected_modify(ModifyKind::Name(mode), path)
    }

    pub fn expected_rename_any(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_rename(RenameMode::Any, path)
    }

    pub fn expected_rename_other(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_rename(RenameMode::Other, path)
    }

    pub fn expected_rename_from(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_rename(RenameMode::From, path)
    }

    pub fn expected_rename_to(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_rename(RenameMode::To, path)
    }

    pub fn expected_rename_both(from: impl AsRef<Path>, to: impl AsRef<Path>) -> ExpectedEvent {
        ExpectedEvent {
            kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            paths: vec![from.as_ref().to_path_buf(), to.as_ref().to_path_buf()],
        }
    }

    pub fn expected_access(kind: AccessKind, path: impl AsRef<Path>) -> ExpectedEvent {
        expected(EventKind::Access(kind), path)
    }

    pub fn expected_open(mode: AccessMode, path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Open(mode), path)
    }

    pub fn expected_open_any(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Open(AccessMode::Any), path)
    }

    pub fn expected_open_other(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Open(AccessMode::Other), path)
    }

    pub fn expected_open_read(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Open(AccessMode::Read), path)
    }

    pub fn expected_open_write(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Open(AccessMode::Write), path)
    }

    pub fn expected_open_exec(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Open(AccessMode::Execute), path)
    }

    pub fn expected_close(mode: AccessMode, path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Close(mode), path)
    }

    pub fn expected_close_any(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Close(AccessMode::Any), path)
    }

    pub fn expected_close_other(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Close(AccessMode::Other), path)
    }

    pub fn expected_close_read(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Close(AccessMode::Read), path)
    }

    pub fn expected_close_write(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Close(AccessMode::Write), path)
    }

    pub fn expected_close_exec(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Close(AccessMode::Execute), path)
    }

    pub fn expected_read(path: impl AsRef<Path>) -> ExpectedEvent {
        expected_access(AccessKind::Read, path)
    }
}
