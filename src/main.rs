extern crate docopt;
extern crate rustc_serialize;
extern crate handlebars;

use std::collections::BTreeMap;
use std::io::{self, BufRead};
use std::process::{Command, Stdio};
use std::thread;

use rustc_serialize::json;
use handlebars::Handlebars;

#[derive(Debug, RustcDecodable)]
struct Args {
    flag_parallel: bool,
    arg_name: String,
    arg_args: Vec<String>,
}

const USAGE: &'static str = r"
JSON version of xargs

Usage:
  jsonargs [--parallel] <name> [<args>...]
  jsonargs (-h | --help)

Options:
  -h --help     Show this message.
  --parallel    Run each command parallel
";

fn main() {
    let args = docopt::Docopt::new(USAGE)
        .and_then(|opt| opt.decode())
        .unwrap_or_else(|e| e.exit());
    let Args { arg_name: name, arg_args: args, flag_parallel: parallel } = args;

    let targs: Vec<Handlebars> = args.into_iter()
        .map(|arg| {
            let mut handlebars = Handlebars::new();
            handlebars.register_template_string("dummy", arg).ok().unwrap();
            handlebars
        })
        .collect();

    let mut childs = Vec::new();

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let decoded: BTreeMap<String, String> = json::decode(&line.unwrap()).unwrap();
        let xargs: Vec<String> = targs.iter()
            .map(|ref targ| targ.render("dummy", &decoded).ok().unwrap())
            .collect();

        let mut child = Command::new(&name)
            .args(&xargs)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        if !parallel {
            child.wait().unwrap();
        } else {
            childs.push(thread::spawn(move || child.wait().unwrap()));
        }
    }

    for child in childs {
        child.join().unwrap();
    }
}
