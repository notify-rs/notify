use backend::{
    prelude::{
        chrono::Utc, futures::{stream::poll_fn, Async, Future, Poll, Sink, Stream}, BackendError,
        BoxedBackend, Capability, Evented, NotifyBackend as Backend, PathBuf,
    },
    stream,
};

use multiqueue::{broadcast_fut_queue, BroadcastFutReceiver, BroadcastFutSender};

use std::{
    fmt, marker::PhantomData, sync::{Arc, Mutex},
};

use tokio::{
    reactor::{Handle, Registration}, runtime::TaskExecutor,
};

/// Convenience return type for methods dealing with backends.
pub type Status = Result<(), BackendError>;

/// Convenience type used in subscription channels.
pub type Sub = Result<stream::Item, Arc<stream::Error>>;

/// Handles a Backend, creating, binding, unbinding, and dropping it as needed.
///
/// A `Backend` is stateless. It takes a set of paths, watches them, and reports events. A `Life`
/// is stateful: it takes a Tokio Handle and TaskExecutor, takes care of wiring up the Backend when
/// needed and taking it down when not, and maintains a consistent interface to its event stream
/// that doesn't die when the Backend is dropped, with event receivers that can be owned safely.
pub struct Life<B: Backend<Item = stream::Item, Error = stream::Error>> {
    driver: Option<Box<Evented>>,
    queue: (BroadcastFutSender<Sub>, BroadcastFutReceiver<Sub>),
    handle: Handle,
    executor: TaskExecutor,
    backend: Arc<Mutex<Option<BoxedBackend>>>,
    registration: Arc<Mutex<Registration>>,
    phantom: PhantomData<B>,
}

impl<B: Backend<Item = stream::Item, Error = stream::Error>> Life<B> {
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
            queue: broadcast_fut_queue(100),
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

    /// Returns a Receiver channel that will get every event and error.
    ///
    /// Be sure to consume it, as leaving events to pile up will eventually block event processing
    /// for all threads and possibly panic. Also be sure to call `.unsubscribe()` or drop it if not
    /// actively needed (anymore) whenever possible.
    fn sub(&self) -> BroadcastFutReceiver<Sub>;

    /// Returns the capabilities of the backend, passed through as-is.
    fn capabilities(&self) -> Vec<Capability>;

    /// Returns the name of the Backend and therefore of this Life.
    fn name(&self) -> &'static str;
}

impl<B: Backend<Item = stream::Item, Error = stream::Error>> LifeTrait for Life<B> {
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

            let wrap = &mut *back.lock().unwrap();
            let back = match wrap {
                // If we don't have a backend anymore, we don't have events either.
                None => return Ok(Async::Ready(None)),
                Some(ref mut b) => b,
            };

            // If the event source is readable, get the backend to read it.
            // The backend will likely have a buffer, so we want to move values into there asap,
            // rather than risk an upstream buffer overflow which would kill the stream.
            if let Async::Ready(ready) = reg.poll_read_ready()? {
                if ready.is_readable() {
                    return back.poll();
                }
            }

            // Otherwise, try for a backend poll anyway, because there might be more events in its
            // internal buffer, and we want to get them all out rather than wait for the next loop.
            return back.poll();
        });

        let mut txs = self.queue.0.clone();
        let mut txe = self.queue.0.clone();
        self.executor.spawn(
            poller
                .for_each(move |mut event| {
                    if event.time.is_none() {
                        event.time = Some(Utc::now());
                    }

                    txs.start_send(Ok(event.clone())).expect(&format!(
                        "Receiver was dropped before Sender was done, failed to send event: {:?}",
                        event
                    ));

                    Ok(())
                })
                .map_err(move |e| {
                    let erc = Arc::new(e);
                    txe.start_send(Err(erc.clone())).expect(&format!(
                        "Receiver was dropped before Sender was done, failed to send error: {:?}",
                        erc
                    ));
                }),
        );

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

    fn sub(&self) -> BroadcastFutReceiver<Sub> {
        self.queue.1.clone()
    }

    fn capabilities(&self) -> Vec<Capability> {
        B::capabilities()
    }

    fn name(&self) -> &'static str {
        B::name()
    }
}

impl<B: Backend<Item = stream::Item, Error = stream::Error>> fmt::Debug for Life<B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct(&format!("Life<{}>", self.name()))
            .field("backend", &self.backend)
            .field("handle", &self.handle)
            .field("registration", &self.registration)
            .finish()
    }
}
