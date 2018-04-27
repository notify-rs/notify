use backend::{prelude::{
    chrono::Utc,
    futures::{
        Future,
        Sink,
        Stream,
        sync::mpsc,
    },
    BackendError,
    BoxedBackend,
    Capability,
    Evented,
    NotifyBackend as Backend,
    PathBuf,
}, stream};

use std::{fmt, marker::PhantomData};
use std::sync::{Arc, Mutex};
use tokio::reactor::{Handle, Registration};
use tokio::runtime::TaskExecutor;

/// Convenience return type for methods dealing with backends.
pub type Status = Result<(), BackendError>;

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

    /// Sets the name of the backend/life if it has not already been set.
    ///
    /// This is more about ease of debugging than anything. Dumping a `Life` item with the `{:?}`
    /// formatter and discovering nothing more useful than `Life { ... }` is not particularly
    /// helpful. With this, `Debug` returns: `Life { name: "Name", ... }`.
    fn with_name(&mut self, name: String);
}

/// The internal structure of binding-related things on a Life.
pub struct BoundBackend {
    // pub backend: Box<Future<Item=(), Error=stream::Error>>,
    pub driver: Box<Evented>,
}

/// Handles a Backend, creating, binding, unbinding, and dropping it as needed.
///
/// A `Backend` is stateless. It takes a set of paths, watches them, and reports events. A `Life`
/// is stateful: it takes a Tokio Handle and TaskExecutor, takes care of wiring up the Backend when
/// needed and taking it down when not, and maintains a consistent interface to its event stream
/// that doesn't die when the Backend is dropped, with event receivers that can be owned safely.
pub struct Life<B: Backend<Item=stream::Item, Error=stream::Error>> {
    name: String,
    bound: Option<BoundBackend>,
    subs: Arc<Mutex<Vec<Option<mpsc::Sender<stream::Item>>>>>,
    handle: Handle,
    executor: TaskExecutor,
    registration: Registration,
    phantom: PhantomData<B>
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> Life<B> {
    /// Internal implementation of `.bind()`.
    #[doc(hidden)]
    fn bind_backend(&mut self, backend: BoxedBackend) -> Status {
        self.unbind()?;

        let driver = backend.driver();
        self.registration.register_with(&driver, &self.handle)?;
        self.bound = Some(BoundBackend { driver });

        let subs = self.subs.clone();

        // TODO: put signaling in there so the backend can be stopped and dropped
        self.executor.spawn(backend.for_each(move |mut event| {
            println!("Inside event: {:?}", event);

            if event.time.is_none() {
                event.time = Some(Utc::now());
            }

            for opt in subs.lock().unwrap().iter_mut() {
                if let Some(sub) = opt {
                    sub.start_send(event.clone())?;
                }
            }

            Ok(())
        }).map_err(|e| {
            println!("Error: {:?}", e)
        }));

        Ok(())
    }

    /// Creates a new, empty life.
    ///
    /// This should only be used with a qualified type, i.e.
    ///
    /// ```no_compile
    /// let life: Life<inotify::Backend> = Life::new(Handle::current());
    /// ```
    pub fn new(handle: Handle, executor: TaskExecutor) -> Self {
        Self {
            name: "".into(),
            bound: None,
            subs: Arc::new(Mutex::new(vec![])),
            handle,
            executor,
            registration: Registration::new(),
            phantom: PhantomData,
        }
    }
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> LifeTrait for Life<B> {
    fn active(&self) -> bool {
        self.bound.is_some()
    }

    fn bind(&mut self, paths: Vec<PathBuf>) -> Status {
        let backend = B::new(paths)?;
        self.bind_backend(backend)
    }

    fn unbind(&mut self) -> Status {
        match self.bound {
            None => return Ok(()),
            Some(ref b) => self.registration.deregister(&b.driver)?
        };

        self.bound = None;
        Ok(())
    }

    fn sub(&self) -> (mpsc::Receiver<stream::Item>, usize) {
        let mut subs = self.subs.lock().unwrap();
        let (tx, rx) = mpsc::channel(100);
        subs.push(Some(tx));
        (rx, subs.len() - 1)
    }

    fn unsub(&self, token: usize) {
        let mut subs = self.subs.lock().unwrap();
        subs[token] = None;
    }

    fn capabilities(&self) -> Vec<Capability> {
        B::capabilities()
    }

    fn with_name(&mut self, name: String) {
        if self.name.len() == 0 {
            self.name = name;
        }
    }
}

impl fmt::Debug for BoundBackend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BoundBackend").finish()
    }
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> fmt::Debug for Life<B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct(&if self.name.len() > 0 {
            format!("Life<{}>", self.name)
        } else { "Life".into() })
            .field("bound", &self.bound)
            .field("subs", &self.subs)
            .field("handle", &self.handle)
            .field("registration", &self.registration)
            .finish()
    }
}
