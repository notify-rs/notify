use announcer::messages::{load_config, save_config, Config, Message};
use notify::{
    event::{DataChange, ModifyKind},
    Error, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::sync::{Arc, Mutex};
use tide::{Body, Response};

const CONFIG_PATH: &str = "config.json";

#[async_std::main]
async fn main() -> tide::Result<()> {
    let config = load_config(CONFIG_PATH).unwrap();
    let config = Arc::new(Mutex::new(config));
    let cloned_config = Arc::clone(&config);

    let mut watcher: RecommendedWatcher =
        Watcher::new_immediate(move |result: Result<Event, Error>| {
            let event = result.unwrap();

            if event.kind == EventKind::Modify(ModifyKind::Any) {
                match load_config(CONFIG_PATH) {
                    Ok(new_config) => *cloned_config.lock().unwrap() = new_config,
                    Err(error) => println!("Error reloading config: {:?}", error),
                }
            }
        })?;

    watcher.watch(CONFIG_PATH, RecursiveMode::Recursive)?;

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
