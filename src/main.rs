extern crate notify;
extern crate tokio;

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
    println!("Acquired manager");

    let events = man.sub();
    println!("Acquired event sub");

    man.builtins();
    man.enliven();
    println!(
        "Prepared built-in backends: {} live of {} available of 2 compiled",
        man.lives.len(),
        man.selectors.len()
    );

    for life in &man.lives {
        println!("==> {:?}", life);
        println!("    backend can: {:?}", life.capabilities());
    }

    let mut args: Vec<String> = env::args().skip(1).collect();
    println!("Retrieved command arguments: {:?}", args);

    if args.is_empty() {
        args.push("src/".into());
        println!("No paths given, adding default path");
    }

    let paths: Vec<PathBuf> = args.iter().map(|s| s.into()).collect();
    println!("Converted args to paths: {:?}", paths);

    man.bind(&paths).unwrap();
    println!("Manager bound: {:?}", man);

    println!("Spawn reporter, filtering on Modify");
    runtime.spawn(events.for_each(|event| {
        match event {
            Err(e) => println!("{:?}", e),
            Ok(e) => if e.kind.is_modify() {
                println!("{:#?}", e);
            },
        };
        // println!("{:?}", event);
        Ok(())
    }));

    println!("Start notify!\n");
    runtime.shutdown_on_idle().wait().unwrap();
}
