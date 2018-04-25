extern crate notify;
extern crate tokio;

use notify::manager::Manager;
use std::{env, path::PathBuf};
use tokio::reactor::Handle;

fn main() {
    let handle = Handle::current();
    println!("Acquired tokio handle: {:?}", handle);

    let mut man = Manager::new(handle);
    man.builtins();
    man.enliven();
    println!("Prepared built-in backends: {} live of {} available of 3 compiled", man.lives.len(), man.selectors.len());

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

    // println!("Handle the backend stream");
    // let b = life.backend().unwrap();
    // let s = b.for_each(|e| {
    //     println!("event {:?}", e);
    //     Ok(())
    // }).map_err(|e| {
    //     println!("error {:?}", e);
    // });

    println!("Run");
    // tokio::run(s);
}
