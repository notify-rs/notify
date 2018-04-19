extern crate notify;
extern crate tokio;

use notify::select::SelectFns;
use tokio::reactor::Handle;

fn main() {
    let handle = Handle::current();
    let mut sel = SelectFns::new(&handle);
    sel.builtins();
}
