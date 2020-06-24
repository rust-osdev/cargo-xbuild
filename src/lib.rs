extern crate cargo_metadata;
#[macro_use]
extern crate error_chain;
extern crate fs2;
#[cfg(any(
    all(target_os = "linux", not(target_env = "musl")),
    target_os = "macos"
))]
extern crate libc;
extern crate rustc_version;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate tempfile;
extern crate toml;
extern crate walkdir;

use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::{env, io, process};

use rustc_version::Channel;

use errors::*;
use rustc::Target;

mod cargo;
mod cli;
mod config;
mod errors;
mod extensions;
mod flock;
mod rustc;
mod sysroot;
mod util;
mod xargo;

pub use cli::{Args, Verbosity};
pub use config::Config;

// We use a different sysroot for Native compilation to avoid file locking
//
// Cross compilation requires `lib/rustlib/$HOST` to match `rustc`'s sysroot,
// whereas Native compilation wants to use a custom `lib/rustlib/$HOST`. If each
// mode has its own sysroot then we avoid sharing that directory and thus file
// locking it.
#[derive(Debug)]
pub enum CompilationMode {
    Cross(Target),
    Native(String),
}

impl CompilationMode {
    fn hash<H>(&self, hasher: &mut H) -> Result<()>
    where
        H: Hasher,
    {
        match *self {
            CompilationMode::Cross(ref target) => target.hash(hasher)?,
            CompilationMode::Native(ref triple) => triple.hash(hasher),
        }

        Ok(())
    }

    /// Returns the condensed target triple (removes any `.json` extension and path components).
    fn triple(&self) -> &str {
        match *self {
            CompilationMode::Cross(ref target) => target.triple(),
            CompilationMode::Native(ref triple) => triple,
        }
    }

    /// Returns the original target triple passed to xargo (perhaps with `.json` extension).
    fn orig_triple(&self) -> &str {
        match *self {
            CompilationMode::Cross(ref target) => target.orig_triple(),
            CompilationMode::Native(ref triple) => triple,
        }
    }

    fn is_native(&self) -> bool {
        match *self {
            CompilationMode::Native(_) => true,
            _ => false,
        }
    }
}

pub fn main_common(command_name: &str) {
    fn show_backtrace() -> bool {
        env::var("RUST_BACKTRACE").as_ref().map(|s| &s[..]) == Ok("1")
    }

    match run(command_name) {
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
        Ok(Some(status)) => {
            if !status.success() {
                process::exit(status.code().unwrap_or(1))
            }
        }
        Ok(None) => {}
    }
}

fn run(command_name: &str) -> Result<Option<ExitStatus>> {
    use cli::Command;

    let (command, args) = cli::args(command_name)?;
    match command {
        Command::Build => Ok(Some(build(args, command_name, None)?)),
        Command::Help => {
            print!(include_str!("help.txt"), command_name = command_name);
            Ok(None)
        }
        Command::Version => {
            writeln!(
                io::stdout(),
                concat!("cargo-xbuild ", env!("CARGO_PKG_VERSION"), "{}"),
                include_str!(concat!(env!("OUT_DIR"), "/commit-info.txt"))
            )
            .unwrap();
            Ok(None)
        }
    }
}

/// Execute a cargo command with cross compiled sysroot crates for custom targets.
///
/// If `crate_config` is provided it will override the values in the `Cargo.toml`.
/// Otherwise the config specified in the `[package.metadata.cargo-xbuild]` section will be used.
pub fn build(args: Args, command_name: &str, crate_config: Option<Config>) -> Result<ExitStatus> {
    let verbose = args.verbose();
    let quiet = args.quiet();
    let meta = rustc::version().map_err(|e| format!("getting rustc version failed: {}", e))?;
    let cd = CurrentDirectory::get()?;
    let config = cargo::config()?;

    let mut cmd = cargo_metadata::MetadataCommand::new();
    if let Some(manifest_path) = args.manifest_path() {
        cmd.manifest_path(manifest_path);
    }

    let metadata = cmd.exec().expect("cargo metadata invocation failed");
    let root = Path::new(&metadata.workspace_root);

    // Fall back to manifest if config not explicitly specified
    let crate_config = crate_config.map(Ok).unwrap_or_else(|| {
        Config::from_metadata(&metadata, args.quiet()).map_err(|e| {
            format!(
                "reading package.metadata.cargo-xbuild section failed: {}",
                e
            )
        })
    })?;

    // We can't build sysroot with stable or beta due to unstable features
    let sysroot = rustc::sysroot(verbose)?;
    let src = match meta.channel {
        Channel::Dev => rustc::Src::from_env().ok_or(
            "The XARGO_RUST_SRC env variable must be set and point to the \
             Rust source directory when working with the 'dev' channel",
        )?,
        Channel::Nightly => {
            if let Some(src) = rustc::Src::from_env() {
                src
            } else {
                sysroot.src()?
            }
        }
        Channel::Stable | Channel::Beta => {
            bail!(
                "The sysroot can't be built for the {:?} channel. \
                 Switch to nightly.",
                meta.channel
            );
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
                Target::new(&triple, &cd, verbose)?.map(CompilationMode::Cross)
            } else {
                Some(CompilationMode::Native(meta.host.clone()))
            }
        } else {
            Some(CompilationMode::Native(meta.host.clone()))
        }
    };

    if let Some(CompilationMode::Native(_)) = cmode {
        eprintln!(
            "WARNING: You're currently building for the host system. This is likely an \
            error and will cause build scripts of dependencies to break.\n\n\

            To build for the target system either pass a `--target` argument or \
            set the build.target configuration key in a `.cargo/config` file.\n",
        );
    }

    if let Some(cmode) = cmode {
        let home = xargo::home(root, &crate_config, quiet)?;
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
        return xargo::run(
            &args,
            &cmode,
            rustflags,
            &home,
            &meta,
            command_name,
            verbose,
        );
    }

    cargo::run(&args, verbose)
}

pub struct CurrentDirectory {
    path: PathBuf,
}

impl CurrentDirectory {
    fn get() -> Result<CurrentDirectory> {
        env::current_dir()
            .chain_err(|| "couldn't get the current directory")
            .map(|cd| CurrentDirectory { path: cd })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}
