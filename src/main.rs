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
    // So basically the problem here... is I'm trying to run the event loop from an event that will
    // not exist without the inner-spawned future (the backend core) running, so nothing ever runs.
    // In some applications, that wouldn't necessarily matter because the loop will be driven by
    // other things anyway e.g. network. But in most applications the Notify event sources will be
    // the main driving forces. So Notify needs to run from itself.
    //
    // This may need more reading/thought, but my main idea is to have two mechanisms:
    //
    // 1. An initial kickstart process employing an exponential growth to kick off futures at timed
    //    intervals starting at 1ms and multiplying by 2 until it gets to 256ms.
    //
    // 2. A timer loop that only runs if no event was received within the last 256ms, to keep the
    //    loop refreshing if needed. This second one might not be needed, tbc.
    //
    // To support at least that second one, but also as a useful feature, needs timestamps to be
    // added to events. Thinking further, while local-filesystem-based backends do not provide this,
    // more advanced backends, especially remote backends, could provide event timestamps themselves
    // so having timestamps in the core Event type would be useful.
    //
    // The idea there is to have an Option<Timestamp>, using whatever timestamp type is most useful
    // and general, but also considering usability (because some processing will need to be done in
    // Notify itself, so an opaque type would be annoying). Backends would have the option to add a
    // timestamp if they know it, otherwise the Life would add the timestamp in on receipt. If a
    // timestamp is already present it will not be modified.
    //
    // Something that could also be useful for debugging at least is event source. For this I'm
    // also considering adding a name() -> String method to the trait, so that a backend can express
    // its name. That would supplant the Life and Selector naming facilities. Event source might
    // also be interesting for later processing e.g. applying things from events based on provenance
    // especially in scenarios where events might come from multiple sources simultaneously aka
    // "partial path set runtime fallback".
    //
    // I'm going to sit on this for a day or two to let the idea settle then pick it back later.

    // tokio::run(events.for_each(|event| {
    //     println!("Event: {:?}", event);
    //     Ok(())
    // }));

    life.unsub(token);
}
