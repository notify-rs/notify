use super::lifecycle::{Life, LifeTrait};
use std::fmt;
use tokio::{reactor::Handle, runtime::TaskExecutor};

#[macro_export]
macro_rules! lifefn {
    ($name:ident < $mod:ty >) => {
        pub fn $name(handle: Handle, executor: TaskExecutor) -> Box<LifeTrait> {
            let l: Life<$mod> = Life::new(handle, executor);
            Box::new(l)
        }
    };
}

#[macro_export]
macro_rules! usefn {
    ($mod:ident => $name:ident) => {
        use $mod;
        lifefn!($name<$mod::Backend>);
    };
}

#[cfg(any(target_os = "linux", target_os = "android"))]
usefn!(inotify => inotify_life);

// #[cfg(any(
//     target_os = "dragonfly",
//     target_os = "freebsd",
//     target_os = "netbsd",
//     target_os = "openbsd",
// ))]
// usefn!(kqueue => kqueue_life);

usefn!(poll => poll_life);

type SelectFn = Fn(Handle, TaskExecutor) -> Box<LifeTrait>;

pub struct Selector<'h> {
    pub f: &'h SelectFn,
    pub name: String, // TODO: perhaps remove? Anyway this entire selector thing has to be reviewed.
}

impl<'h> fmt::Debug for Selector<'h> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Selector {{ (Handle, TaskExecutor) -> Box<Life<{}>> }}",
            self.name
        )
    }
}
