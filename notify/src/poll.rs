//! Generic Watcher implementation based on polling
//!
//! Checks the `watch`ed paths periodically to detect changes. This implementation only uses
//! Rust stdlib APIs and should work on all of the platforms it supports.

use crate::{EventHandler, RecursiveMode, Watcher, Config};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use data::{DataBuilder, WatchData};
mod data {
    use crate::{
        event::{CreateKind, DataChange, Event, EventKind, MetadataKind, ModifyKind, RemoveKind},
        EventHandler,
    };
    use filetime::FileTime;
    use std::{
        cell::RefCell,
        collections::{hash_map::RandomState, HashMap},
        fmt::{self, Debug},
        fs::{self, File, Metadata},
        hash::{BuildHasher, Hasher},
        io::{self, Read},
        path::{Path, PathBuf},
        time::Instant,
    };
    use walkdir::WalkDir;

    /// Builder for [`WatchData`] & [`PathData`].
    pub(super) struct DataBuilder {
        emitter: EventEmitter,

        // TODO: May allow user setup their custom BuildHasher / BuildHasherDefault
        // in future.
        build_hasher: Option<RandomState>,

        // current timestamp for building Data.
        now: Instant,
    }

    impl DataBuilder {
        pub(super) fn new<F>(event_handler: F, compare_content: bool) -> Self
        where
            F: EventHandler,
        {
            Self {
                emitter: EventEmitter::new(event_handler),
                build_hasher: compare_content.then(RandomState::default),
                now: Instant::now(),
            }
        }

        /// Update internal timestamp.
        pub(super) fn update_timestamp(&mut self) {
            self.now = Instant::now();
        }

        /// Create [`WatchData`].
        ///
        /// This function will return `Err(_)` if can not retrieve metadata from
        /// the path location. (e.g., not found).
        pub(super) fn build_watch_data(
            &self,
            root: PathBuf,
            is_recursive: bool,
        ) -> Option<WatchData> {
            WatchData::new(self, root, is_recursive)
        }

        /// Create [`PathData`].
        fn build_path_data(&self, meta_path: &MetaPath) -> PathData {
            PathData::new(self, meta_path)
        }
    }

    impl Debug for DataBuilder {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("DataBuilder")
                .field("build_hasher", &self.build_hasher)
                .field("now", &self.now)
                .finish()
        }
    }

    #[derive(Debug)]
    pub(super) struct WatchData {
        // config part, won't change.
        root: PathBuf,
        is_recursive: bool,

        // current status part.
        all_path_data: HashMap<PathBuf, PathData>,
    }

    impl WatchData {
        /// Scan filesystem and create a new `WatchData`.
        ///
        /// # Side effect
        ///
        /// This function may send event by `data_builder.emitter`.
        fn new(data_builder: &DataBuilder, root: PathBuf, is_recursive: bool) -> Option<Self> {
            // If metadata read error at `root` path, it will emit
            // a error event and stop to create the whole `WatchData`.
            //
            // QUESTION: inconsistent?
            //
            // When user try to *CREATE* a watch by `poll_watcher.watch(root, ..)`,
            // if `root` path hit an io error, then watcher will reject to
            // create this new watch.
            //
            // This may inconsistent with *POLLING* a watch. When watcher
            // continue polling, io error at root path will not delete
            // a existing watch. polling still working.
            //
            // So, consider a config file may not exists at first time but may
            // create after a while, developer cannot watch it.
            //
            // FIXME: Can we always allow to watch a path, even file not
            // found at this path?
            if let Err(e) = fs::metadata(&root) {
                data_builder.emitter.emit_io_err(e, &root);
                return None;
            }

            let all_path_data =
                Self::scan_all_path_data(data_builder, root.clone(), is_recursive).collect();

            Some(Self {
                root,
                is_recursive,
                all_path_data,
            })
        }

        /// Rescan filesystem and update this `WatchData`.
        ///
        /// # Side effect
        ///
        /// This function may emit event by `data_builder.emitter`.
        pub(super) fn rescan(&mut self, data_builder: &mut DataBuilder) {
            // scan current filesystem.
            for (path, new_path_data) in
                Self::scan_all_path_data(data_builder, self.root.clone(), self.is_recursive)
            {
                let old_path_data = self
                    .all_path_data
                    .insert(path.clone(), new_path_data.clone());

                // emit event
                let event =
                    PathData::compare_to_event(path, old_path_data.as_ref(), Some(&new_path_data));
                if let Some(event) = event {
                    data_builder.emitter.emit_ok(event);
                }
            }

            // scan for disappeared paths.
            let mut disappeared_paths = Vec::new();
            for (path, path_data) in self.all_path_data.iter() {
                if path_data.last_check < data_builder.now {
                    disappeared_paths.push(path.clone());
                }
            }

            // remove disappeared paths
            for path in disappeared_paths {
                let old_path_data = self.all_path_data.remove(&path);

                // emit event
                let event = PathData::compare_to_event(path, old_path_data.as_ref(), None);
                if let Some(event) = event {
                    data_builder.emitter.emit_ok(event);
                }
            }
        }

        /// Get all `PathData` by given configuration.
        ///
        /// # Side Effect
        ///
        /// This function may emit some IO Error events by `data_builder.emitter`.
        fn scan_all_path_data(
            data_builder: &'_ DataBuilder,
            root: PathBuf,
            is_recursive: bool,
        ) -> impl Iterator<Item = (PathBuf, PathData)> + '_ {
            // WalkDir return only one entry if root is a file (not a folder),
            // so we can use single logic to do the both file & dir's jobs.
            //
            // See: https://docs.rs/walkdir/2.0.1/walkdir/struct.WalkDir.html#method.new
            WalkDir::new(root)
                .follow_links(true)
                .max_depth(Self::dir_scan_depth(is_recursive))
                .into_iter()
                //
                // QUESTION: should we ignore IO Error?
                //
                // current implementation ignore some IO error, e.g.,
                //
                // - `.filter_map(|entry| entry.ok())`
                // - all read error when hashing
                //
                // but the code also interest with `fs::metadata()` error and
                // propagate to event handler. It may not consistent.
                //
                // FIXME: Should we emit all IO error events? Or ignore them all?
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| match entry.metadata() {
                    Ok(metadata) => {
                        let path = entry.into_path();

                        let meta_path = MetaPath::from_parts_unchecked(path, metadata);
                        let data_path = data_builder.build_path_data(&meta_path);

                        Some((meta_path.into_path(), data_path))
                    }
                    Err(e) => {
                        // emit event.
                        let path = entry.into_path();
                        data_builder.emitter.emit_io_err(e, path);

                        None
                    }
                })
        }

        fn dir_scan_depth(is_recursive: bool) -> usize {
            if is_recursive {
                usize::max_value()
            } else {
                1
            }
        }
    }

    /// Stored data for a one path locations.
    ///
    /// See [`WatchData`] for more detail.
    #[derive(Debug, Clone)]
    struct PathData {
        /// File updated time.
        mtime: i64,

        /// Content's hash value, only available if user request compare file
        /// contents and read successful.
        hash: Option<u64>,

        /// Checked time.
        last_check: Instant,
    }

    impl PathData {
        /// Create a new `PathData`.
        fn new(data_builder: &DataBuilder, meta_path: &MetaPath) -> PathData {
            let metadata = meta_path.metadata();

            PathData {
                mtime: FileTime::from_last_modification_time(metadata).seconds(),
                hash: data_builder
                    .build_hasher
                    .as_ref()
                    .filter(|_| metadata.is_file())
                    .and_then(|build_hasher| {
                        Self::get_content_hash(build_hasher, meta_path.path()).ok()
                    }),

                last_check: data_builder.now,
            }
        }

        /// Get hash value for the data content in given file `path`.
        fn get_content_hash(build_hasher: &RandomState, path: &Path) -> io::Result<u64> {
            let mut hasher = build_hasher.build_hasher();
            let mut file = File::open(path)?;
            let mut buf = [0; 512];

            loop {
                let n = match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(len) => len,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                };

                hasher.write(&buf[..n]);
            }

            Ok(hasher.finish())
        }

        /// Get [`Event`] by compare two optional [`PathData`].
        fn compare_to_event<P>(
            path: P,
            old: Option<&PathData>,
            new: Option<&PathData>,
        ) -> Option<Event>
        where
            P: Into<PathBuf>,
        {
            match (old, new) {
                (Some(old), Some(new)) => {
                    if new.mtime > old.mtime {
                        Some(EventKind::Modify(ModifyKind::Metadata(
                            MetadataKind::WriteTime,
                        )))
                    } else if new.hash != old.hash {
                        Some(EventKind::Modify(ModifyKind::Data(DataChange::Any)))
                    } else {
                        None
                    }
                }
                (None, Some(_new)) => Some(EventKind::Create(CreateKind::Any)),
                (Some(_old), None) => Some(EventKind::Remove(RemoveKind::Any)),
                (None, None) => None,
            }
            .map(|event_kind| Event::new(event_kind).add_path(path.into()))
        }
    }

    /// Compose path and its metadata.
    ///
    /// This data structure designed for make sure path and its metadata can be
    /// transferred in consistent way, and may avoid some duplicated
    /// `fs::metadata()` function call in some situations.
    #[derive(Debug)]
    pub(super) struct MetaPath {
        path: PathBuf,
        metadata: Metadata,
    }

    impl MetaPath {
        /// Create `MetaPath` by given parts.
        ///
        /// # Invariant
        ///
        /// User must make sure the input `metadata` are associated with `path`.
        fn from_parts_unchecked(path: PathBuf, metadata: Metadata) -> Self {
            Self { path, metadata }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn metadata(&self) -> &Metadata {
            &self.metadata
        }

        fn into_path(self) -> PathBuf {
            self.path
        }
    }

    /// Thin wrapper for outer event handler, for easy to use.
    struct EventEmitter(
        // Use `RefCell` to make sure `emit()` only need shared borrow of self (&self).
        // Use `Box` to make sure EventEmitter is Sized.
        Box<RefCell<dyn EventHandler>>,
    );

    impl EventEmitter {
        fn new<F: EventHandler>(event_handler: F) -> Self {
            Self(Box::new(RefCell::new(event_handler)))
        }

        /// Emit single event.
        fn emit(&self, event: crate::Result<Event>) {
            self.0.borrow_mut().handle_event(event);
        }

        /// Emit event.
        fn emit_ok(&self, event: Event) {
            self.emit(Ok(event))
        }

        /// Emit io error event.
        fn emit_io_err<E, P>(&self, err: E, path: P)
        where
            E: Into<io::Error>,
            P: Into<PathBuf>,
        {
            self.emit(Err(crate::Error::io(err.into()).add_path(path.into())))
        }
    }
}

/// Polling based `Watcher` implementation.
/// 
/// By default scans through all files and checks for changed entries based on their change date.
/// Can also be changed to perform file content change checks.
/// 
/// See [Config] for more details.
#[derive(Debug)]
pub struct PollWatcher {
    watches: Arc<Mutex<HashMap<PathBuf, WatchData>>>,
    data_builder: Arc<Mutex<DataBuilder>>,
    want_to_stop: Arc<AtomicBool>,
    delay: Duration,
}

impl PollWatcher {
    /// Create a new [PollWatcher], configured as needed.
    pub fn new<F: EventHandler>(
        event_handler: F,
        config: Config,
    ) -> crate::Result<PollWatcher> {
        let data_builder = DataBuilder::new(event_handler, config.compare_contents());

        let poll_watcher = PollWatcher {
            watches: Default::default(),
            data_builder: Arc::new(Mutex::new(data_builder)),
            want_to_stop: Arc::new(AtomicBool::new(false)),
            delay: config.poll_interval(),
        };

        poll_watcher.run();

        Ok(poll_watcher)
    }

    fn run(&self) {
        let watches = Arc::clone(&self.watches);
        let data_builder = Arc::clone(&self.data_builder);
        let want_to_stop = Arc::clone(&self.want_to_stop);
        let delay = self.delay;

        let _ = thread::Builder::new()
            .name("notify-rs poll loop".to_string())
            .spawn(move || {
                loop {
                    if want_to_stop.load(Ordering::SeqCst) {
                        break;
                    }

                    // HINT: Make sure always lock in the same order to avoid deadlock.
                    //
                    // FIXME: inconsistent: some place mutex poison cause panic,
                    // some place just ignore.
                    if let (Ok(mut watches), Ok(mut data_builder)) =
                        (watches.lock(), data_builder.lock())
                    {
                        data_builder.update_timestamp();

                        let vals = watches.values_mut();
                        for watch_data in vals {
                            watch_data.rescan(&mut data_builder);
                        }
                    }

                    // QUESTION: `actual_delay == process_time + delay`. Is it intended to?
                    //
                    // If not, consider fix it to:
                    //
                    // ```rust
                    // let still_need_to_delay = delay.checked_sub(data_builder.now.elapsed());
                    // if let Some(delay) = still_need_to_delay {
                    //     thread::sleep(delay);
                    // }
                    // ```
                    thread::sleep(delay);
                }
            });
    }

    /// Watch a path location.
    ///
    /// QUESTION: this function never return an Error, is it as intend?
    /// Please also consider the IO Error event problem.
    fn watch_inner(&mut self, path: &Path, recursive_mode: RecursiveMode) {
        // HINT: Make sure always lock in the same order to avoid deadlock.
        //
        // FIXME: inconsistent: some place mutex poison cause panic, some place just ignore.
        if let (Ok(mut watches), Ok(mut data_builder)) =
            (self.watches.lock(), self.data_builder.lock())
        {
            data_builder.update_timestamp();

            let watch_data =
                data_builder.build_watch_data(path.to_path_buf(), recursive_mode.is_recursive());

            // if create watch_data successful, add it to watching list.
            if let Some(watch_data) = watch_data {
                watches.insert(path.to_path_buf(), watch_data);
            }
        }
    }

    /// Unwatch a path.
    ///
    /// Return `Err(_)` if given path has't be monitored.
    fn unwatch_inner(&mut self, path: &Path) -> crate::Result<()> {
        // FIXME: inconsistent: some place mutex poison cause panic, some place just ignore.
        self.watches
            .lock()
            .unwrap()
            .remove(path)
            .map(|_| ())
            .ok_or_else(crate::Error::watch_not_found)
    }
}

impl Watcher for PollWatcher {
    /// Create a new [PollWatcher].
    fn new<F: EventHandler>(event_handler: F, config: Config) -> crate::Result<Self> {
        Self::new(event_handler, config)
    }

    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> crate::Result<()> {
        self.watch_inner(path, recursive_mode);

        Ok(())
    }

    fn unwatch(&mut self, path: &Path) -> crate::Result<()> {
        self.unwatch_inner(path)
    }

    fn kind() -> crate::WatcherKind {
        crate::WatcherKind::PollWatcher
    }
}

impl Drop for PollWatcher {
    fn drop(&mut self) {
        self.want_to_stop.store(true, Ordering::Relaxed);
    }
}

#[test]
fn poll_watcher_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<PollWatcher>();
}
