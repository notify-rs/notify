extern crate notify;
extern crate tokio;

use notify::manager::Manager;
use std::{env, path::PathBuf};
use tokio::prelude::Stream;
use tokio::runtime::Runtime;

fn main() {
    let runtime = Runtime::new().unwrap();
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

    let (events, token) = life.sub();
    println!("Acquired event sub: {}", token);

    println!("Run");
    tokio::run(events.for_each(|event| {
        println!("Event: {:?}", event);
        Ok(())
    }));

    life.unsub(token);
}
