extern crate cargo_metadata;
#[macro_use]
extern crate error_chain;
extern crate fs2;
#[cfg(any(all(target_os = "linux", not(target_env = "musl")), target_os = "macos"))]
extern crate libc;
extern crate rustc_version;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate tempdir;
extern crate toml;
extern crate walkdir;
extern crate xargo_lib;

use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::{env, io, process};

use rustc_version::Channel;

use xargo_lib::errors::*;
use xargo_lib::rustc::Target;

use xargo_lib::cargo;
use xargo_lib::cli;
use xargo_lib::config;
use xargo_lib::errors;
use xargo_lib::extensions;
use xargo_lib::flock;
use xargo_lib::rustc;
use xargo_lib::sysroot;
use xargo_lib::util;
use xargo_lib::xargo;

use xargo_lib::CurrentDirectory;
use xargo_lib::CompilationMode;

const HELP: &str = include_str!("help.txt");



pub fn main() {
    fn show_backtrace() -> bool {
        env::var("RUST_BACKTRACE").as_ref().map(|s| &s[..]) == Ok("1")
    }

    match run() {
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

fn run() -> Result<Option<ExitStatus>> {
    use cli::Command;

    let (command, args) = cli::args("xbuild")?;
    match command {
        Command::Build => Ok(Some(build(args)?)),
        Command::Help => {
            print!("{}", HELP);
            Ok(None)
        }
        Command::Version => {
            writeln!(
                io::stdout(),
                concat!("cargo-xbuild ", env!("CARGO_PKG_VERSION"), "{}"),
                include_str!(concat!(env!("OUT_DIR"), "/commit-info.txt"))
            ).unwrap();
            Ok(None)
        }
    }
}

fn build(args: cli::Args) -> Result<(ExitStatus)> {
    let verbose = args.verbose();
    let meta = rustc::version();
    let cd = CurrentDirectory::get()?;
    let config = cargo::config()?;

    let metadata =
        cargo_metadata::metadata(args.manifest_path()).expect("cargo metadata invocation failed");
    let root = Path::new(&metadata.workspace_root);
    let crate_config = config::Config::from_metadata(&metadata)
        .map_err(|_| "parsing package.metadata.cargo-xbuild section failed")?;

    // We can't build sysroot with stable or beta due to unstable features
    let sysroot = rustc::sysroot(verbose)?;
    let src = match meta.channel {
        Channel::Dev => rustc::Src::from_env().ok_or(
            "The XARGO_RUST_SRC env variable must be set and point to the \
             Rust source directory when working with the 'dev' channel",
        )?,
        Channel::Nightly => if let Some(src) = rustc::Src::from_env() {
            src
        } else {
            sysroot.src()?
        },
        Channel::Stable | Channel::Beta => {
            writeln!(
                io::stderr(),
                "WARNING: the sysroot can't be built for the {:?} channel. \
                 Switch to nightly.",
                meta.channel
            ).ok();
            return cargo::run(&args, verbose);
        }
    };

    let cmode = if let Some(triple) = args.target() {
        if triple == meta.host {
            Some(CompilationMode::Native(meta.host.clone()))
        } else {
            Target::new(triple, &cd, verbose)?.map(CompilationMode::Cross)
        }
    } else {
        if let Some(ref config) = config {
            if let Some(triple) = config.target()? {
                Target::new(triple, &cd, verbose)?.map(CompilationMode::Cross)
            } else {
                Some(CompilationMode::Native(meta.host.clone()))
            }
        } else {
            Some(CompilationMode::Native(meta.host.clone()))
        }
    };

    if let Some(cmode) = cmode {
        let home = xargo::home(root, &crate_config)?;
        let rustflags = cargo::rustflags(config.as_ref(), cmode.triple())?;

        sysroot::update(
            &cmode,
            &home,
            &root,
            &crate_config,
            &rustflags,
            &meta,
            &src,
            &sysroot,
            verbose,
        )?;
        return xargo::run(&args, &cmode, rustflags, &home, &meta, false, verbose);
    }

    cargo::run(&args, verbose)
}
