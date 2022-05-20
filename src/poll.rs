//! Generic Watcher implementation based on polling
//!
//! Checks the `watch`ed paths periodically to detect changes. This implementation only uses
//! Rust stdlib APIs and should work on all of the platforms it supports.

use super::event::*;
use super::{Error, EventHandler, RecursiveMode, Result, Watcher};
use filetime::FileTime;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::Metadata;
use std::hash::BuildHasher;
use std::hash::Hasher;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use std::{fs, io};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
struct PathData {
    mtime: i64,
    hash: Option<u64>,
    last_check: Instant,
}

#[derive(Debug)]
struct WatchData {
    is_recursive: bool,
    paths: HashMap<PathBuf, PathData>,
}

/// Polling based `Watcher` implementation
pub struct PollWatcher {
    event_handler: Arc<Mutex<dyn EventHandler>>,
    watches: Arc<Mutex<HashMap<PathBuf, WatchData>>>,
    open: Arc<AtomicBool>,
    delay: Duration,
    compare_contents: bool,
}

/// General purpose configuration for [`PollWatcher`] specifically.  Can be used to tune
/// this watcher differently than the other platform specific ones.
#[derive(Debug, Clone)]
pub struct PollWatcherConfig {
    /// Interval between each rescan attempt.  This can be extremely expensive for large
    /// file trees so it is recommended to measure and tune accordingly.
    pub poll_interval: Duration,

    /// Optional feature that will evaluate the contents of changed files to determine if
    /// they have indeed changed using a fast hashing algorithm.  This is especially important
    /// for pseudo filesystems like those on Linux under /sys and /proc which are not obligated
    /// to respect any other filesystem norms such as modification timestamps, file sizes, etc.
    /// By enabling this feature, performance will be significantly impacted as all files will
    /// need to be read and hashed at each `poll_interval`.
    pub compare_contents: bool,
}

impl Default for PollWatcherConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(30),
            compare_contents: false,
        }
    }
}

impl Debug for PollWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PollWatcher")
            .field("event_handler", &Arc::as_ptr(&self.watches))
            .field("watches", &self.watches)
            .field("open", &self.open)
            .field("delay", &self.delay)
            .field("compare_contents", &self.compare_contents)
            .finish()
    }
}

fn emit_event(event_handler: &Mutex<dyn EventHandler>, res: Result<Event>) {
    if let Ok(mut guard) = event_handler.lock() {
        let f: &mut dyn EventHandler = &mut *guard;
        f.handle_event(res);
    }
}

impl PathData {
    pub fn collect<BH: BuildHasher>(
        path: &Path,
        metadata: &Metadata,
        build_hasher: Option<&BH>,
        last_check: Instant,
    ) -> Self {
        let mtime = FileTime::from_last_modification_time(metadata).seconds();
        let hash = metadata
            .is_file()
            .then(|| build_hasher.and_then(|bh| Self::hash_file(path, bh).ok()))
            .flatten();
        Self {
            mtime,
            hash,
            last_check,
        }
    }

    fn hash_file<P: AsRef<Path>, BH: BuildHasher>(path: P, build_hasher: &BH) -> io::Result<u64> {
        let mut hasher = build_hasher.build_hasher();
        let mut file = fs::File::open(path)?;
        let mut buf = [0; 512];
        loop {
            let n = match file.read(&mut buf) {
                Ok(0) => break,
                Ok(len) => len,
                Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            hasher.write(&buf[..n]);
        }
        Ok(hasher.finish())
    }

    pub fn detect_change(&self, other: &PathData) -> Option<EventKind> {
        if self.mtime > other.mtime {
            Some(EventKind::Modify(ModifyKind::Metadata(
                MetadataKind::WriteTime,
            )))
        } else if self.hash != other.hash {
            Some(EventKind::Modify(ModifyKind::Data(DataChange::Any)))
        } else {
            None
        }
    }
}

impl PollWatcher {
    /// Create a new [PollWatcher], configured as needed.
    pub fn with_config<F: EventHandler>(
        event_handler: F,
        config: PollWatcherConfig,
    ) -> Result<PollWatcher> {
        let mut p = PollWatcher {
            event_handler: Arc::new(Mutex::new(event_handler)),
            watches: Arc::new(Mutex::new(HashMap::new())),
            open: Arc::new(AtomicBool::new(true)),
            delay: config.poll_interval,
            compare_contents: config.compare_contents,
        };
        p.run();
        Ok(p)
    }

    fn run(&mut self) {
        let watches = self.watches.clone();
        let open = self.open.clone();
        let delay = self.delay;
        let build_hasher = self.compare_contents.then(RandomState::default);
        let event_handler = self.event_handler.clone();
        let event_handler = move |res| emit_event(&event_handler, res);

        let _ = thread::Builder::new()
            .name("notify-rs poll loop".to_string())
            .spawn(move || {
                // In order of priority:
                // TODO: handle metadata events
                // TODO: handle renames
                // TODO: DRY it up

                loop {
                    if !open.load(Ordering::SeqCst) {
                        break;
                    }

                    if let Ok(mut watches) = watches.lock() {
                        let current_time = Instant::now();

                        for (
                            watch,
                            &mut WatchData {
                                is_recursive,
                                ref mut paths,
                            },
                        ) in watches.iter_mut()
                        {
                            match fs::metadata(watch) {
                                Err(e) => {
                                    let err = Err(Error::io(e).add_path(watch.clone()));
                                    event_handler(err);
                                    continue;
                                }
                                Ok(metadata) => {
                                    // this is a file type watching point
                                    if !metadata.is_dir() {
                                        let path_data = PathData::collect(
                                            watch,
                                            &metadata,
                                            build_hasher.as_ref(),
                                            current_time,
                                        );

                                        // Update `path_data` for this watching point. In file
                                        // type watching point, only has single one path need
                                        // to update.
                                        match paths.insert(watch.clone(), path_data.clone()) {
                                            // `old_path_data` not exists, this is a new path
                                            None => {
                                                let kind = EventKind::Create(CreateKind::Any);
                                                let ev = Event::new(kind).add_path(watch.clone());
                                                event_handler(Ok(ev));
                                            }

                                            // path already exists, need further check to see
                                            // what's the difference.
                                            Some(old_path_data) => {
                                                if let Some(kind) =
                                                    path_data.detect_change(&old_path_data)
                                                {
                                                    let ev =
                                                        Event::new(kind).add_path(watch.clone());
                                                    event_handler(Ok(ev));
                                                }
                                            }
                                        }

                                    // this is a dir type watching point
                                    } else {
                                        let depth =
                                            if is_recursive { usize::max_value() } else { 1 };
                                        for entry in WalkDir::new(watch)
                                            .follow_links(true)
                                            .max_depth(depth)
                                            .into_iter()
                                            .filter_map(|e| e.ok())
                                        {
                                            let path = entry.path();

                                            // TODO: duplicate logic, considering refactor following lines to a function.
                                            match entry.metadata() {
                                                Err(e) => {
                                                    let err = Error::io(e.into())
                                                        .add_path(path.to_path_buf());
                                                    event_handler(Err(err));
                                                }
                                                Ok(m) => {
                                                    let path_data = PathData::collect(
                                                        path,
                                                        &m,
                                                        build_hasher.as_ref(),
                                                        current_time,
                                                    );
                                                    match paths.insert(
                                                        path.to_path_buf(),
                                                        path_data.clone(),
                                                    ) {
                                                        None => {
                                                            let kind =
                                                                EventKind::Create(CreateKind::Any);
                                                            let ev = Event::new(kind)
                                                                .add_path(path.to_path_buf());
                                                            event_handler(Ok(ev));
                                                        }
                                                        Some(old_path_data) => {
                                                            if let Some(kind) = path_data
                                                                .detect_change(&old_path_data)
                                                            {
                                                                // TODO add new mtime as attr
                                                                let ev = Event::new(kind)
                                                                    .add_path(path.to_path_buf());
                                                                event_handler(Ok(ev));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // clear out all paths which not updated by this round.
                        for (_, &mut WatchData { ref mut paths, .. }) in watches.iter_mut() {
                            // find which paths should be removed in this watching point.
                            let mut removed = Vec::new();
                            for (path, &PathData { last_check, .. }) in paths.iter() {
                                if last_check < current_time {
                                    let ev = Event::new(EventKind::Remove(RemoveKind::Any))
                                        .add_path(path.clone());
                                    event_handler(Ok(ev));
                                    removed.push(path.clone());
                                }
                            }

                            // remove actually.
                            for path in removed {
                                (*paths).remove(&path);
                            }
                        }
                    }

                    thread::sleep(delay);
                }
            });
    }

    fn watch_inner(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        let build_hasher = self.compare_contents.then(RandomState::default);

        if let Ok(mut watches) = self.watches.lock() {
            let current_time = Instant::now();

            let watch = path.to_owned();

            match fs::metadata(path) {
                Err(e) => {
                    let err = Error::io(e).add_path(watch);
                    emit_event(&self.event_handler, Err(err));
                }
                Ok(metadata) => {
                    let mut paths = HashMap::new();

                    if !metadata.is_dir() {
                        let path_data =
                            PathData::collect(path, &metadata, build_hasher.as_ref(), current_time);
                        paths.insert(watch.clone(), path_data);
                    } else {
                        let depth = if recursive_mode.is_recursive() {
                            usize::max_value()
                        } else {
                            1
                        };
                        for entry in WalkDir::new(watch.clone())
                            .follow_links(true)
                            .max_depth(depth)
                            .into_iter()
                            .filter_map(|e| e.ok())
                        {
                            let path = entry.path();

                            match entry.metadata() {
                                Err(e) => {
                                    let err = Error::io(e.into()).add_path(path.to_path_buf());
                                    emit_event(&self.event_handler, Err(err));
                                }
                                Ok(m) => {
                                    let path_data = PathData::collect(
                                        path,
                                        &m,
                                        build_hasher.as_ref(),
                                        current_time,
                                    );
                                    paths.insert(path.to_path_buf(), path_data);
                                }
                            }
                        }
                    }

                    watches.insert(
                        watch,
                        WatchData {
                            is_recursive: recursive_mode.is_recursive(),
                            paths,
                        },
                    );
                }
            }
        }
        Ok(())
    }

    fn unwatch_inner(&mut self, path: &Path) -> Result<()> {
        if (*self.watches).lock().unwrap().remove(path).is_some() {
            Ok(())
        } else {
            Err(Error::watch_not_found())
        }
    }
}

impl Watcher for PollWatcher {
    /// Create a new [PollWatcher].
    ///
    /// The default poll frequency is 30 seconds.
    /// Use [with_config] to manually set the poll frequency.
    fn new<F: EventHandler>(event_handler: F) -> Result<Self> {
        Self::with_config(event_handler, PollWatcherConfig::default())
    }

    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        self.watch_inner(path, recursive_mode)
    }

    fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.unwatch_inner(path)
    }

    fn kind() -> crate::WatcherKind {
        crate::WatcherKind::PollWatcher
    }
}

impl Drop for PollWatcher {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Relaxed);
    }
}

#[test]
fn poll_watcher_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<PollWatcher>();
}
