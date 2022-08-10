// Imagine this is a web app that remembers information about audio messages.
// It has a config.json file that acts as a database,
// you can edit the configuration and the app will pick up changes without the need to restart it.
// This concept is known as hot-reloading.
use hot_reload_tide::messages::{load_config, Config};
use notify::{Error, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tide::{Body, Response};

const CONFIG_PATH: &str = "config.json";

// Because we're running a web server we need a runtime,
// for more information on async runtimes, please check out [async-std](https://github.com/async-rs/async-std)
#[async_std::main]
async fn main() -> tide::Result<()> {
    let config = load_config(CONFIG_PATH).unwrap();

    // We wrap the data a mutex under an atomic reference counted pointer
    // to guarantee that the config won't be read and written to at the same time.
    // To learn about how that works,
    // please check out the [Fearless Concurrency](https://doc.rust-lang.org/book/ch16-00-concurrency.html) chapter of the Rust book.
    let config = Arc::new(Mutex::new(config));
    let cloned_config = Arc::clone(&config);

    // We listen to file changes by giving Notify
    // a function that will get called when events happen
    let mut watcher =
        // To make sure that the config lives as long as the function
        // we need to move the ownership of the config inside the function
        // To learn more about move please read [Using move Closures with Threads](https://doc.rust-lang.org/book/ch16-01-threads.html?highlight=move#using-move-closures-with-threads)
        RecommendedWatcher::new(move |result: Result<Event, Error>| {
            let event = result.unwrap();

            if event.kind.is_modify() {
                match load_config(CONFIG_PATH) {
                    Ok(new_config) => *cloned_config.lock().unwrap() = new_config,
                    Err(error) => println!("Error reloading config: {:?}", error),
                }
            }
        },notify::Config::default())?;

    watcher.watch(Path::new(CONFIG_PATH), RecursiveMode::Recursive)?;

    // We set up a web server using [Tide](https://github.com/http-rs/tide)
    let mut app = tide::with_state(config);

    app.at("/messages").get(get_messages);
    app.at("/message/:name").get(get_message);
    app.listen("127.0.0.1:8080").await?;

    Ok(())
}

type Request = tide::Request<Arc<Mutex<Config>>>;

async fn get_messages(req: Request) -> tide::Result {
    let mut res = Response::new(200);
    let config = &req.state().lock().unwrap();
    let body = Body::from_json(&config.messages)?;
    res.set_body(body);
    Ok(res)
}

async fn get_message(req: Request) -> tide::Result {
    let mut res = Response::new(200);

    let name: String = req.param("name")?.parse()?;
    let config = &req.state().lock().unwrap();
    let value = config.messages.get(&name);

    let body = Body::from_json(&value)?;
    res.set_body(body);
    Ok(res)
}
