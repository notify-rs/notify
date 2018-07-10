use super::{
    lifecycle::{LifeTrait, Status}, selector::{self, Selector},
};
use backend::prelude::{BackendError, PathBuf};
use std::fmt;
use tokio::{reactor::Handle, runtime::TaskExecutor};

pub struct Manager<'f> {
    pub handle: Handle,
    pub executor: TaskExecutor,
    pub selectors: Vec<Selector<'f>>,
    pub lives: Vec<Box<LifeTrait + 'f>>,
}

impl<'f> Manager<'f> {
    pub fn new(handle: Handle, executor: TaskExecutor) -> Self {
        Self {
            handle,
            executor,
            selectors: vec![],
            lives: vec![],
        }
    }

    pub fn add(&mut self, f: Selector<'f>) {
        self.selectors.push(f)
    }

    pub fn builtins(&mut self) {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        self.add(Selector {
            f: &selector::inotify_life,
            name: "Inotify".into(),
        });

        // #[cfg(any(
        //     target_os = "dragonfly",
        //     target_os = "freebsd",
        //     target_os = "netbsd",
        //     target_os = "openbsd",
        // ))]
        // self.add(Selector { f: &selector::kqueue_life, name: "Kqueue".into() });

        self.add(Selector {
            f: &selector::poll_life,
            name: "Poll".into(),
        });
    }

    pub fn enliven(&mut self) {
        let mut lives = vec![];

        for sel in &self.selectors {
            let mut l = (sel.f)(self.handle.clone(), self.executor.clone());

            if !l.capabilities().is_empty() {
                let sub = l.sub();
                sub.unsubscribe();
                lives.push(l);
            }
        }

        self.lives = lives;
    }

    // TODO: figure out how to handle per-path errors
    // Most of the ::bind() code is temporary to get something fuctional
    // but should not be considered final!

    pub fn bind(&mut self, paths: &[PathBuf]) -> Status {
        // takes: a set of paths
        // returns: a Status
        //
        // tries to allocate paths to each life in order.
        // - if paths fail on one life, it tries again with a smaller subset,
        // helped by the pathed error hints.
        // - as soon as it's got a good life, it tries to fit the remaining paths,
        // if any, into other lifes (if any).
        // - then it finishes looping lives and disables others if they're live
        // (i.e. it clears everything that remains so there's no lives running
        // on "old" paths)

        let mut err = None;
        for life in &mut self.lives {
            println!("Trying {:?}", life);
            match life.bind(paths) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    println!("Got error(s): {:?}", e);
                    for ie in e.as_error_vec() {
                        match ie {
                            be @ BackendError::NonExistent(_) => return Err(be.into()),
                            be => {
                                err = Some(be.clone());
                            }
                        }
                    }
                }
            }
        }

        return match err {
            Some(e) => Err(e.into()),
            None => Err(BackendError::Unavailable(Some("No backend available".into())).into()),
        }
    }

    fn bind_to_life(&mut self, index: usize, paths: &[PathBuf]) -> (Status, Vec<PathBuf>, Vec<PathBuf>) {
        // takes: index into self.lives, some paths
        // gives: a status for the life we just tried, and two lists of paths:
        // 1/ paths that could have succeeded
        // 2/ paths that definitely failed
        // (if Status is success then both of these are empty)
        //
        // we do that by parsing the error for pathed errors

        (Ok(()), vec![], vec![])
    }

    #[cfg_attr(feature = "cargo-clippy", allow(borrowed_box))]
    pub fn active(&mut self) -> Option<&mut Box<LifeTrait + 'f>> {
        for life in &mut self.lives {
            if life.active() {
                return Some(life);
            }
        }

        None
    }
}

impl<'f> fmt::Debug for Manager<'f> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Manager")
            .field("handle", &self.handle)
            .field("selectors", &self.selectors)
            .field("lives", &self.lives)
            .finish()
    }
}
