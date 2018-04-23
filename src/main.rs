extern crate notify;
extern crate tokio;

use notify::manager::Manager;
use std::path::PathBuf;
use tokio::reactor::Handle;
use tokio::prelude::*;

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
    
    let path: PathBuf = "/opt/notify-test".into();
    println!("Let us bind to {:?}", path);
    man.bind(vec![path]).unwrap();

    let life = man.active().unwrap();
    println!("Bound {:?}", life);

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
