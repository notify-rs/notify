use super::lifecycle::{Life, LifeTrait};
use tokio::reactor::Handle;

#[cfg(any(
    target_os = "linux",
    target_os = "android",
))]
use inotify;

#[cfg(any(
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
use kqueue;

use poll_tree;

#[cfg(any(
    target_os = "linux",
    target_os = "android",
))]
fn inotify_life(handle: &Handle) -> Life<inotify::Backend> {
    Life::new(handle)
}

fn poll_life(handle: &Handle) -> Life<poll_tree::Backend> {
    Life::new(handle)
}

fn lives<'h>(handle: &'h Handle) -> Vec<Box<LifeTrait + 'h>> {
    let mut lives: Vec<Box<LifeTrait>> = vec![];

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
    ))] lives.push(Box::new(inotify_life(handle)));

    lives.push(Box::new(poll_life(handle)));

    lives
}
