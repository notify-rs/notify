use backend::{prelude::{
    futures::{
        Stream,
        stream::Forward,
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
use tokio::reactor::{Handle, Registration};

/*
stream -> ForEach(|event| {
    for channel in self.channels.iter_mut() {
        let (tx,_) = channel;
        tx.send(event.clone())
    }
}

})

channels = Vec<(tx, rx)>

i.e. single-producer, multi-consumer cloned channels
*/

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

    /// Returns the capabilities of the backend, passed through as-is.
    fn capabilities(&self) -> Vec<Capability>;

    /// Sets the name of the backend/life if it has not already been set.
    ///
    /// This is more about ease of debugging than anything. Dumping a `Life` item with the `{:?}`
    /// formatter and discovering nothing more useful than `Life { bound: None }` is not
    /// particularly helpful. With this, `Debug` returns: `Life<Name> { ... }`.
    fn with_name(&mut self, name: String);
}

/// The internal structure of binding-related things on a Life.
pub struct BoundBackend {
    pub backend: Forward<BoxedBackend, mpsc::UnboundedSender<stream::Item>>,
    pub channel: mpsc::UnboundedReceiver<stream::Item>,
    pub driver: Box<Evented>,
}

/// Handles a Backend, creating, binding, unbinding, and dropping it as needed.
///
/// A `Backend` is stateless. It takes a set of paths, watches them, and reports events. A `Life`
/// is stateful: it takes a Tokio Handle, takes care of wiring up the Backend when needed and
/// taking it down when not, and maintains a consistent interface to its event stream that doesn't
/// die when the Backend is dropped, with event receivers that can be owned safely.
pub struct Life<B: Backend<Item=stream::Item, Error=stream::Error>> {
    bound: Option<BoundBackend>,
    handle: Handle,
    name: Option<String>,
    registration: Registration,
    phantom: PhantomData<B>
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> Life<B> {
    /// Internal implementation of `.bind()`.
    #[doc(hidden)]
    fn bind_backend(&mut self, boxback: BoxedBackend) -> Status {
        // TODO: unbind after binding the new one, to avoid missing on events
        self.unbind()?;

        let driver = boxback.driver();
        let (tx, channel) = mpsc::unbounded();
        let backend = boxback.forward(tx);

        self.registration.register_with(&driver, &self.handle)?;
        self.bound = Some(BoundBackend { backend, channel, driver });
        Ok(())
    }

    /// Creates a new, empty life.
    ///
    /// This should only be used with a qualified type, i.e.
    ///
    /// ```no_compile
    /// let life: Life<inotify::Backend> = Life::new(Handle::current());
    /// ```
    pub fn new(handle: Handle) -> Self {
        Self {
            bound: None,
            handle: handle,
            name: None,
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

    fn capabilities(&self) -> Vec<Capability> {
        B::capabilities()
    }

    fn with_name(&mut self, name: String) {
        if self.name.is_none() {
            self.name = Some(name);
        }
    }
}

impl fmt::Debug for BoundBackend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BoundBackend")
            .field("backend", &self.backend)
            .field("channel", &self.channel)
            .finish()
    }
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> fmt::Debug for Life<B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct(&match self.name {
            Some(ref name) => format!("Life<{}>", name),
            None => "Life".into()
        }).field("bound", &self.bound).finish()
    }
}
