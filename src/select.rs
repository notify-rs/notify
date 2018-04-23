use std::fmt;
use super::lifecycle::{Life, LifeTrait};
use tokio::reactor::Handle;

#[macro_export]
macro_rules! lifefn {
    ($name:ident<$mod:ty>) => {
    fn $name<'h>(handle: &'h Handle) -> Box<LifeTrait + 'h> {
        let mut l: Life<$mod> = Life::new(handle);
        l.with_name(stringify!($mod).trim_right_matches("::Backend").into());
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

pub struct Selector<'h> {
    pub f: &'h SelectFn<'h>,
    pub name: String,
}

impl<'h> fmt::Debug for Selector<'h> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Selector {{ Handle -> Box<Life<{}>> }}", self.name)
    }
}

impl<'h> From<&'h SelectFn<'h>> for Selector<'h> {
    fn from(f: &'h SelectFn<'h>) -> Selector<'h> {
        Selector { f: f, name: "Anonymous".into() }
    }
}

#[derive(Debug)]
pub struct SelectFns<'f> {
    handle: &'f Handle,
    fns: Vec<Selector<'f>>
}

impl<'f> SelectFns<'f> {
    pub fn new(handle: &'f Handle) -> Self {
        Self {
            handle,
            fns: vec![]
        }
    }

    pub fn add(&mut self, f: Selector<'f>) {
        self.fns.push(f)
    }

    pub fn builtins(&mut self) {
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
        ))]
        self.add(Selector { f: &inotify_life, name: "Inotify".into() });

        #[cfg(any(
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
        ))]
        self.add(Selector { f: &kqueue_life, name: "Kqueue".into() });

        self.add(Selector { f: &poll_life, name: "Poll".into() });
    }

    pub fn lives(&self) -> Vec<Box<LifeTrait + 'f>> {
        let mut lives: Vec<Box<LifeTrait>> = vec![];

        for f in self.fns.iter() {
            let mut l = (f.f)(self.handle);
            l.with_name(f.name.clone());
            lives.push(l);
        }

        lives
    }
}
