use super::lifecycle::{Life, LifeTrait};
use tokio::reactor::Handle;

#[macro_export]
macro_rules! lifefn {
    ($name:ident<$mod:ty>) => {
    fn $name<'h>(handle: &'h Handle) -> Box<LifeTrait + 'h> {
        let l: Life<$mod> = Life::new(handle);
        Box::new(l)
    }}
}

#[macro_export]
macro_rules! usefn {
    ($mod:ident => $name:ident) => {
        use $mod;
        lifefn!($name<$mod::Backend>);
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
))]
usefn!(inotify => inotify_life);

#[cfg(any(
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
usefn!(kqueue => kqueue_life);

usefn!(poll_tree => poll_life);

type SelectFn<'h> = Fn(&'h Handle) -> Box<LifeTrait + 'h>;

pub struct SelectFns<'f> {
    handle: &'f Handle,
    fns: Vec<&'f SelectFn<'f>>
}

impl<'f> SelectFns<'f> {
    pub fn new(handle: &'f Handle) -> Self {
        Self {
            handle,
            fns: vec![]
        }
    }

    pub fn add(&mut self, f: &'f SelectFn<'f>) {
        self.fns.push(f)
    }

    pub fn builtins(&mut self) {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
        ))]
        self.add(&inotify_life);

        #[cfg(any(
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
        ))]
        self.add(&kqueue_life);

        self.add(&poll_life);
    }

    pub fn lives(&self) -> Vec<Box<LifeTrait + 'f>> {
        let mut lives: Vec<Box<LifeTrait>> = vec![];

        for f in self.fns.iter() {
            lives.push(f(self.handle));
        }

        lives
    }
}
