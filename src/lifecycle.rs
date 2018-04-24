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

pub struct Life<B: Backend<Item=stream::Item, Error=stream::Error>> {
    backend: Option<Forward<BoxedBackend, mpsc::UnboundedSender<stream::Item>>>,
    channel: Option<mpsc::UnboundedReceiver<stream::Item>>,
    driver: Option<Box<Evented>>,
    handle: Handle,
    name: Option<String>,
    registration: Registration,
    phantom: PhantomData<B>
}

pub type Status = Result<(), BackendError>;

pub trait LifeTrait: fmt::Debug {
    fn active(&self) -> bool;
    fn bind(&mut self, paths: Vec<PathBuf>) -> Status;
    fn unbind(&mut self) -> Status;
    fn capabilities(&self) -> Vec<Capability>;
    fn with_name(&mut self, name: String);
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> Life<B> {
    fn bind_backend(&mut self, backend: BoxedBackend) -> Status {
        // TODO: unbind after binding the new one, to avoid missing on events
        self.unbind()?;

        let d = backend.driver();
        let (tx, rx) = mpsc::unbounded();
        let b = backend.forward(tx);

        self.registration.register_with(&d, &self.handle)?;

        self.driver = Some(d);
        self.backend = Some(b);
        self.channel = Some(rx);
        Ok(())
    }

    pub fn new(handle: Handle) -> Self {
        Self {
            backend: None,
            channel: None,
            driver: None,
            handle: handle,
            name: None,
            registration: Registration::new(),
            phantom: PhantomData,
        }
    }
}

impl<B: Backend<Item=stream::Item, Error=stream::Error>> LifeTrait for Life<B> {
    fn active(&self) -> bool {
        self.backend.is_some()
    }

    fn bind(&mut self, paths: Vec<PathBuf>) -> Status {
        let backend = B::new(paths)?;
        self.bind_backend(backend)
    }

    fn unbind(&mut self) -> Status {
        match self.driver {
            None => Ok(()),
            Some(ref d) => self.registration.deregister(d).map(|_| ()),
        }.map_err(|e| e.into())
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
