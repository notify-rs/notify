use backend::prelude::{
    BackendError,
    BoxedBackend,
    NotifyBackend as Backend,
    PathBuf,
};

use tokio::reactor::{Handle, Registration};

pub struct Life<'h, B: Backend> {
    backend: Option<B>,
    handle: &'h Handle,
    registration: Registration,
}

pub type Status = Result<(), BackendError>;

impl<'h, B> Life<'h, B> where B: Backend {
    pub fn new(handle: &'h Handle) -> Self {
        Self {
            backend: None,
            handle: handle,
            registration: Registration::new(),
        }
    }

    pub fn bind(&mut self, paths: Vec<PathBuf>) -> Status
    where Box<B>:From<BoxedBackend> {
        let backend = B::new(paths)?;
        self.bind_backend(backend.into())
    }

    fn bind_backend(&mut self, backend: Box<B>) -> Status {
        self.unbind()?;

        let b = *backend;
        self.registration.register_with(&b, self.handle).map(|_| {
            self.backend = Some(b);
        }).map_err(|e| e.into())
    }

    pub fn unbind(&mut self) -> Status {
        match self.backend {
            None => Ok(()),
            Some(ref b) => self.registration.deregister(b).map(|_| ()),
        }.map_err(|e| e.into())
    }
}

impl<'h, B> Drop for Life<'h, B> where B: Backend {
    fn drop(&mut self) {
        self.unbind().unwrap();
    }
}