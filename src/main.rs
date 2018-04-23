extern crate notify;
extern crate tokio;

use notify::select::SelectFns;
use tokio::reactor::Handle;

fn main() {
    let handle = Handle::current();
    let mut sel = SelectFns::new(&handle);
    sel.builtins();

    let lives = sel.lives();
    for life in lives {
        println!("{:?}", life);
        println!("Capabilities: {:?}\n", life.capabilities());
    }
}
