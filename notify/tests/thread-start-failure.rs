#![cfg(target_os = "linux")]

use notify::{Config, ErrorKind, INotifyWatcher, PollWatcher, Watcher};
use std::{process::Command, sync::mpsc};

#[test]
fn thread_start_failures_are_reported() {
    // Linux does not enforce RLIMIT_NPROC for root.
    if unsafe { libc::getuid() } == 0 {
        eprintln!("skipping thread limit test as root");
        return;
    }

    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "thread_start_failure_child",
            "--nocapture",
        ])
        .output()
        .expect("run thread limit test in a child process");
    assert!(
        output.status.success(),
        "child failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "run by thread_start_failures_are_reported in a separate process"]
fn thread_start_failure_child() {
    // Limit only this child process, after the test harness has started its worker thread.
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` points to an initialized rlimit for the duration of the call.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NPROC, &limit) }, 0);

    check_thread_start_failure::<PollWatcher>();
    check_thread_start_failure::<INotifyWatcher>();
}

fn check_thread_start_failure<W: Watcher>() {
    let (tx, rx) = mpsc::channel();
    let error = W::new(tx, Config::default())
        .err()
        .expect("watcher construction must fail when it cannot start its thread");
    assert!(
        matches!(error.kind, ErrorKind::Io(ref cause) if cause.raw_os_error() == Some(libc::EAGAIN)),
        "{:?}: {error:?}",
        W::kind()
    );
    assert!(
        matches!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected)),
        "{:?} must release its event handler on construction failure",
        W::kind()
    );
}
