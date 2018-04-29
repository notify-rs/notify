use backend::{prelude::{
    chrono::Utc,
    futures::{
        Async,
        Future,
        Poll,
        Sink,
        Stream,
        sync::mpsc,
        stream::poll_fn,
    },
    BackendError,
    BoxedBackend,
    Capability,
    Evented,
    NotifyBackend as Backend,
    PathBuf,
}, stream};

use std::{
    collections::HashMap,
    fmt,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use tokio::{
    reactor::{Handle, Registration},
    runtime::TaskExecutor,
};

/// Convenience return type for methods dealing with backends.
pub type Status = Result<(), BackendError>;

/// Handles a Backend, creating, binding, unbinding, and dropping it as needed.
///
/// A `Backend` is stateless. It takes a set of paths, watches them, and reports events. A `Life`
/// is stateful: it takes a Tokio Handle and TaskExecutor, takes care of wiring up the Backend when
/// needed and taking it down when not, and maintains a consistent interface to its event stream
/// that doesn't die when the Backend is dropped, with event receivers that can be owned safely.
pub struct Life<B: Backend<Item=stream::Item, Error=stream::Error>> {
    driver: Option<Box<Evented>>,
    subs: Arc<Mutex<HashMap<usize, mpsc::Sender<stream::Item>>>>,
    handle: Handle,
    executor: TaskExecutor,
    backend: Arc<Mutex<Option<BoxedBackend>>>,
    registration: Arc<Mutex<Registration>>,
    phantom: PhantomData<B>
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> Life<B> {
    /// Creates a new, empty life.
    ///
    /// This should only be used with a qualified type, i.e.
    ///
    /// ```no_compile
    /// let life: Life<inotify::Backend> = Life::new(Handle::current());
    /// ```
    pub fn new(handle: Handle, executor: TaskExecutor) -> Self {
        Self {
            backend: Arc::new(Mutex::new(None)),
            driver: None,
            subs: Arc::new(Mutex::new(HashMap::new())),
            handle,
            executor,
            registration: Arc::new(Mutex::new(Registration::new())),
            phantom: PhantomData,
        }
    }
}

/// Convenience trait to be able to pass generic `Life<T>` around.
///
/// There will only ever be one implementation of the trait, but specifying `Box<LifeTrait>` is
/// more convenient than requiring that every consumer be generic over `T`.
pub trait LifeTrait: fmt::Debug {
    /// Returns whether there is a bound backend on this Life.
    fn active(&self) -> bool;

    /// Attempts to bind a backend to a set of paths.
    fn bind(&mut self, paths: Vec<PathBuf>) -> Status;

    /// Attempts to unbind a backend.
    ///
    /// Technically this can fail, but failure should be more or less fatal as it probably
    /// indicates a larger failure. However, one can retry the unbind.
    fn unbind(&mut self) -> Status;

    /// Returns a Receiver channel that will get every event.
    ///
    /// Be sure to consume it, as leaving events to pile up will eventually block event processing
    /// for all threads. The second element of the tuple is a token to be used with `.unsub()` to
    /// cancel the sender half of the channel. Whenever possible, do so before dropping to avoid
    /// potentially bricking operations.
    fn sub(&self) -> (mpsc::Receiver<stream::Item>, usize);

    /// Cancels a channel obtained with `.sub()`.
    fn unsub(&self, token: usize);

    /// Returns the capabilities of the backend, passed through as-is.
    fn capabilities(&self) -> Vec<Capability>;

    /// Returns the name of the Backend and therefore of this Life.
    fn name(&self) -> &'static str;
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> LifeTrait for Life<B> {
    fn active(&self) -> bool {
        self.driver.is_some()
    }

    fn bind(&mut self, paths: Vec<PathBuf>) -> Status {
        let backend = B::new(paths)?;
        self.unbind()?;

        let driver = backend.driver();
        let reg = self.registration.lock().unwrap();
        reg.register_with(&driver, &self.handle)?;
        self.driver = Some(driver);

        let mut back = self.backend.lock().unwrap();
        *back = Some(backend);

        let back = self.backend.clone();
        let reg = self.registration.clone();
        let poller = poll_fn(move || -> Poll<Option<stream::Item>, stream::Error> {
            let reg = reg.lock().unwrap();
            match reg.poll_read_ready() {
                Err(e) => Err(e.into()),
                Ok(async) => match async {
                    Async::NotReady => Ok(Async::NotReady),
                    Async::Ready(ready) => if ready.is_readable() {
                        match *back.lock().unwrap() {
                            None => Ok(Async::Ready(None)),
                            Some(ref mut backend) => backend.poll()
                        }
                    } else {
                        Ok(Async::NotReady)
                    }
                }
            }
        });

        let subs = self.subs.clone();

        self.executor.spawn(poller.for_each(move |mut event| {
            if event.time.is_none() {
                event.time = Some(Utc::now());
            }

            for sub in subs.lock().unwrap().values_mut() {
                sub.start_send(event.clone())?;
            }

            Ok(())
        }).map_err(|e| {
            println!("Error: {:?}", e) // TODO proper error handling
        }));

        Ok(())
    }

    fn unbind(&mut self) -> Status {
        match self.driver {
            None => return Ok(()),
            Some(ref d) => {
                let mut reg = self.registration.lock().unwrap();
                reg.deregister(d)?
            }
        };

        self.driver = None;

        let mut back = self.backend.lock().unwrap();
        *back = None;

        Ok(())
    }

    fn sub(&self) -> (mpsc::Receiver<stream::Item>, usize) {
        let mut subs = self.subs.lock().unwrap();
        let (tx, rx) = mpsc::channel(100);
        let token = subs.len();
        subs.insert(token, tx);
        (rx, token)
    }

    fn unsub(&self, token: usize) {
        let mut subs = self.subs.lock().unwrap();
        subs.remove(&token);
    }

    fn capabilities(&self) -> Vec<Capability> {
        B::capabilities()
    }

    fn name(&self) -> &'static str {
        B::name()
    }
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> fmt::Debug for Life<B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct(&format!("Life<{}>", self.name()))
            .field("backend", &self.backend)
            .field("subs", &self.subs)
            .field("handle", &self.handle)
            .field("registration", &self.registration)
            .finish()
    }
}
