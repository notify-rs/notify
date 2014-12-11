#![feature(phase)]
#[phase(plugin, link)] extern crate log;

use std::fmt;

#[test]
fn it_works() {
}

pub struct Event {
  pub name: Path,
  pub op: Op,
}

pub enum Op {
  Create,
  Write,
  Remove,
  Rename,
  Chmod,
}

impl fmt::Show for Op {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> fmt::Result {
    match self {
      &Op::Create  => "Create",
      &Op::Write   => "Write",
      &Op::Remove  => "Remove",
      &Op::Rename  => "Rename",
      &Op::Chmod   => "Chmod",
    }.fmt(f)
  }
}
