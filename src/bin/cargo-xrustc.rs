extern crate xargo_lib;

use std::process::ExitStatus;
use std::io::Write;
use std::{env, io, process};

const HELP: &str = include_str!("help.txt");

pub fn main() {
    fn show_backtrace() -> bool {
        env::var("RUST_BACKTRACE").as_ref().map(|s| &s[..]) == Ok("1")
    }

    match xargo_lib::run("xrustc", HELP, true) {
        Err(e) => {
            let stderr = io::stderr();
            let mut stderr = stderr.lock();

            writeln!(stderr, "error: {}", e).ok();

            for e in e.iter().skip(1) {
                writeln!(stderr, "caused by: {}", e).ok();
            }

            if show_backtrace() {
                if let Some(backtrace) = e.backtrace() {
                    writeln!(stderr, "{:?}", backtrace).ok();
                }
            } else {
                writeln!(stderr, "note: run with `RUST_BACKTRACE=1` for a backtrace").ok();
            }

            process::exit(1)
        }
        Ok(Some(status)) => if !status.success() {
            process::exit(status.code().unwrap_or(1))
        },
        Ok(None) => {}
    }
}