use std::{env, process::Command};

const FREEBSD_INOTIFY_MIN_MAJOR: u32 = 15;
const FREEBSD_VERSION_COMMAND: &str = "/bin/freebsd-version";

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(notify_freebsd_inotify)");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("freebsd") {
        return;
    }

    let feature_enabled = env::var_os("CARGO_FEATURE_FREEBSD_INOTIFY").is_some();
    // freebsd-version describes the host, so only use it for native builds.
    let native_build = matches!(
        (env::var("HOST"), env::var("TARGET")),
        (Ok(host), Ok(target)) if host == target
    );
    if native_build {
        println!("cargo::rerun-if-changed={FREEBSD_VERSION_COMMAND}");
    }
    let version_supported = native_build
        && freebsd_major_version().is_some_and(|major| major >= FREEBSD_INOTIFY_MIN_MAJOR);

    if feature_enabled || version_supported {
        println!("cargo::rustc-cfg=notify_freebsd_inotify");
    }
}

fn freebsd_major_version() -> Option<u32> {
    let output = Command::new(FREEBSD_VERSION_COMMAND)
        .arg("-u")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    std::str::from_utf8(&output.stdout)
        .ok()?
        .trim()
        .split('.')
        .next()?
        .parse()
        .ok()
}
