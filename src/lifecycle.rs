use backend::{prelude::{
    BackendError,
    BoxedBackend,
    Capability,
    NotifyBackend as Backend,
    PathBuf,
}, stream};

use std::{fmt, marker::PhantomData};
use tokio::reactor::{Handle, Registration};

pub struct Life<'h, B: Backend<Item=stream::Item, Error=stream::Error>> {
    backend: Option<BoxedBackend>,
    handle: &'h Handle,
    name: Option<String>,
    registration: Registration,
    phantom: PhantomData<B>
}

pub type Status = Result<(), BackendError>;

pub trait LifeTrait: fmt::Debug {
    fn bind(&mut self, paths: Vec<PathBuf>) -> Status;
    fn unbind(&mut self) -> Status;
    fn capabilities(&self) -> Vec<Capability>;
    fn with_name(&mut self, name: String);
}

impl<'h, B: Backend<Item=stream::Item, Error=stream::Error>> Life<'h, B> {
    fn bind_backend(&mut self, backend: BoxedBackend) -> Status {
        self.unbind()?;

        self.registration.register_with(&backend, self.handle).map(|_| {
            self.backend = Some(backend);
        }).map_err(|e| e.into())
    }

    pub fn new(handle: &'h Handle) -> Self {
        Self {
            backend: None,
            handle: handle,
            name: None,
            registration: Registration::new(),
            phantom: PhantomData,
        }
    }
}

impl<'h, B: Backend<Item=stream::Item, Error=stream::Error>> LifeTrait for Life<'h, B> {
    fn bind(&mut self, paths: Vec<PathBuf>) -> Status {
        let backend = B::new(paths)?;
        self.bind_backend(backend)
    }

    fn unbind(&mut self) -> Status {
        match self.backend {
            None => Ok(()),
            Some(ref b) => self.registration.deregister(b).map(|_| ()),
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

impl<'h, B: Backend<Item=stream::Item, Error=stream::Error>> fmt::Debug for Life<'h, B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.name {
            Some(ref name) => write!(f, "Life<{}> {{ backend: {:?} }}", name, self.backend),
            None => write!(f, "Life {{ backend: {:?} }}", self.backend)
        }
    }
}