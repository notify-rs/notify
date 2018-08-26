use super::{
    lifecycle::{LifeTrait, Status, Sub}, selector::{self, Selector},
};
use backend::prelude::{BackendErrorWrap, PathBuf};
use multiqueue::{broadcast_fut_queue, BroadcastFutReceiver, BroadcastFutSender};
use std::fmt;
use tokio::{
    prelude::{Future, Sink, Stream}, reactor::Handle, runtime::TaskExecutor,
};

pub struct Manager<'selector_fn> {
    pub handle: Handle,
    pub executor: TaskExecutor,
    pub selectors: Vec<Selector<'selector_fn>>,
    pub lives: Vec<Box<LifeTrait + 'selector_fn>>,
    queue: (BroadcastFutSender<Sub>, BroadcastFutReceiver<Sub>),
    // add here a Arc<Set<PathBuf>>
    // idea is that this would be updated in block, all at once, instead of
    // adding or removing entries. Processors get an immut owned reference
    // (an arc clone) of the set, then on change they're send another
    // reference to replace their own copy (not a copy). When all processors
    // have dropped the old ref, the memory is reclaimed.
}

impl<'selector_fn> Manager<'selector_fn> {
    pub fn new(handle: Handle, executor: TaskExecutor) -> Self {
        Self {
            handle,
            executor,
            selectors: vec![],
            lives: vec![],
            queue: broadcast_fut_queue(100),
        }
    }

    pub fn add(&mut self, f: Selector<'selector_fn>) {
        self.selectors.push(f)
    }

    // sketch for processors:
    //
    // they live from the moment they're needed to the moment they're not
    // often that will be the entirety of the program
    // i.e. they're very much stateful
    //
    // prelims (processor declares):
    // - whether it will operate on one backend's output or many/all
    // - what capabilities it needs
    // - what capabilities it provides
    //
    // methods:
    //   - here's a new arc clone of watched paths
    //   - finish up
    //
    // inputs:
    // - stream of events
    // - instruction channel
    //
    // outputs:
    // - stream of events
    // - instructions
    //   - watch this
    //   - unwatch this

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
                let mut maintx = self.queue.0.clone();

                // TODO: Find a way to do this properly without using for_each
                // all the time. Surely forward() or send_all() would be better!
                let pipe = sub.for_each(move |event| {
                    maintx.start_send(event).unwrap_or_else(|_| {
                        panic!(
                            "Receiver was dropped before Sender was done, failed to forward event"
                        )
                    });

                    Ok(())
                });

                self.executor.spawn(pipe.map_err(|_| {}));
                lives.push(l);
            }
        }

        self.lives = lives;
    }

    pub fn sub(&self) -> BroadcastFutReceiver<Sub> {
        self.queue.1.clone()
    }

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

        let mut final_err = None;
        let mut remaining = paths.to_vec();
        for life_index in 0..(self.lives.len()) {
            if remaining.is_empty() {
                if let Some(life) = self.lives.get_mut(life_index) {
                    if life.active() {
                        println!("Unbinding life that's not needed anymore: {:?}", life);
                        life.unbind().ok();
                    }
                }
                continue;
            }

            match self.bind_to_life(life_index, &remaining) {
                Ok(_) => {
                    println!("Binding succeeded entirely, finishing up");
                    remaining = vec![];
                    continue;
                }
                Err((err, passes, fails)) => {
                    if passes.is_empty() {
                        println!("Binding failed, skipping to next life\n{:?}", err);
                        final_err = Some(err.clone());
                        continue;
                    } else {
                        println!("Binding may have partially succeeded, trying again");
                        match self.bind_to_life(life_index, &passes) {
                            Ok(_) => {
                                println!("Binding succeeded partially, continuing");
                                remaining = fails.clone();
                                continue;
                            }
                            Err((err, _, _)) => {
                                // TODO: continue the cycle recursively instead of ignoring hints
                                // on second pass and bailing. Would also have to detect cycles?
                                println!("2nd try failed, skipping to next life\n{:?}", err);
                                final_err = Some(err.clone());
                                continue;
                            }
                        }
                    }
                }
            }
        }

        if remaining.is_empty() {
            if let Some(err) = final_err {
                Err(err)
            } else {
                Ok(())
            }
        } else {
            unreachable!();
        }
    }

    fn bind_to_life(
        &mut self,
        index: usize,
        paths: &[PathBuf],
    ) -> Result<(), (BackendErrorWrap, Vec<PathBuf>, Vec<PathBuf>)> {
        // takes: index into self.lives, some paths
        // gives: a status for the life we just tried, and two lists of paths:
        // 1/ paths that could have succeeded
        // 2/ paths that definitely failed
        //
        // we do that by parsing the error for pathed errors

        let life = self.lives
            .get_mut(index)
            .expect("bind_to_life was given a bad index, something is very wrong");

        println!(
            "Attempting to bind {} paths to life {:?}",
            paths.len(),
            life
        );

        let err = match life.bind(paths) {
            Ok(_) => return Ok(()),
            Err(e) => e,
        };

        println!("Got errors: {:?}", err);

        let failed = match err {
            e @ BackendErrorWrap::General(_) | e @ BackendErrorWrap::All(_) => {
                return Err((e, vec![], paths.to_vec()))
            }
            BackendErrorWrap::Single(_, ref paths) => paths.clone(),
            BackendErrorWrap::Multiple(ref tups) => tups.iter()
                .flat_map(|(_, ref paths)| paths.clone())
                .collect(),
        };

        let mut fails = vec![];
        let mut passes = vec![];

        for path in paths {
            if failed.contains(path) {
                fails.push(path.clone());
            } else {
                passes.push(path.clone());
            }
        }

        Err((err, passes, fails))
    }

    #[cfg_attr(feature = "cargo-clippy", allow(borrowed_box))]
    pub fn active(&mut self) -> Option<&mut Box<LifeTrait + 'selector_fn>> {
        for life in &mut self.lives {
            if life.active() {
                return Some(life);
            }
        }

        None
    }
}

impl<'selector_fn> fmt::Debug for Manager<'selector_fn> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Manager")
            .field("handle", &self.handle)
            .field("selectors", &self.selectors)
            .field("lives", &self.lives)
            .finish()
    }
}
