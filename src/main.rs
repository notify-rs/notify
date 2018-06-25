extern crate notify;
extern crate tokio;

use notify::backend::prelude::EventKind;
use notify::manager::Manager;
use std::{env, path::PathBuf};
use tokio::prelude::{Future, Stream};
use tokio::runtime::Runtime;

fn main() {
    let mut runtime = Runtime::new().unwrap();
    println!("Initialised tokio runtime");

    let handle = runtime.reactor().clone();
    println!("Acquired tokio handle");

    let executor = runtime.executor();
    println!("Acquired tokio executor");

    let mut man = Manager::new(handle, executor);
    man.builtins();
    man.enliven();
    println!("Prepared built-in backends: {} live of {} available of 2 compiled", man.lives.len(), man.selectors.len());

    for life in man.lives.iter() {
        println!("==> {:?}", life);
        println!("    backend can: {:?}", life.capabilities());
    }

    let mut args: Vec<String> = env::args().skip(1).collect();
    println!("Retrieved command arguments: {:?}", args);

    if args.len() == 0 {
        args.push("/opt/notify-test".into());
        println!("No paths given, adding default path");
    }

    let paths: Vec<PathBuf> = args.iter().map(|s| s.into()).collect();
    println!("Converted args to paths: {:?}", paths);

    man.bind(paths).unwrap();
    println!("Manager bound: {:?}", man);

    let life = man.active().unwrap();
    println!("Life bound: {:?}", life);

    let events = life.sub();
    println!("Acquired event sub");

    // Why a chrono timestamp and not a stdlib `SystemTime`? Because the expected use for this is
    // with external authoritative times, which will be represented in ISO8601 or similar, and will
    // not make sense as a system time. There is no _local_ filechange API I know of that provides
    // timestamps for changes... likely because it is assumed latency will not be an issue locally.

    println!("Spawn reporter, filtering on Modify");
    runtime.spawn(events.for_each(|event| {
        match event {
            Err(e) => println!("{:?}", e),
            Ok(e) => match e.kind {
                EventKind::Modify(_) => println!("{:#?}", e),
                _ => {}
            }
        };
        // println!("{:?}", event);
        Ok(())
    }));

    println!("Start notify!\n");
    runtime.shutdown_on_idle().wait().unwrap();
}
