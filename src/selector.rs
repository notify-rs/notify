use std::fmt;
use super::lifecycle::{Life, LifeTrait};
use tokio::reactor::Handle;

#[macro_export]
macro_rules! lifefn {
    ($name:ident<$mod:ty>) => {
    pub fn $name(handle: Handle) -> Box<LifeTrait> {
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

// #[cfg(any(
//     target_os = "dragonfly",
//     target_os = "freebsd",
//     target_os = "netbsd",
//     target_os = "openbsd",
// ))]
// usefn!(kqueue => kqueue_life);

usefn!(poll_tree => poll_life);

type SelectFn = Fn(Handle) -> Box<LifeTrait>;

pub struct Selector<'h> {
    pub f: &'h SelectFn,
    pub name: String,
}

impl<'h> fmt::Debug for Selector<'h> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Selector {{ Handle -> Box<Life<{}>> }}", self.name)
    }
}
