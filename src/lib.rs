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

use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::{env, io, process};

use rustc_version::Channel;

use errors::*;
use rustc::Target;

pub mod cargo;
pub mod cli;
pub mod config;
pub mod errors;
pub mod extensions;
pub mod flock;
pub mod rustc;
pub mod sysroot;
pub mod util;
pub mod xargo;

pub struct CurrentDirectory {
    path: PathBuf,
}

impl CurrentDirectory {
    pub fn get() -> Result<CurrentDirectory> {
        env::current_dir()
            .chain_err(|| "couldn't get the current directory")
            .map(|cd| CurrentDirectory { path: cd })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

// We use a different sysroot for Native compilation to avoid file locking
//
// Cross compilation requires `lib/rustlib/$HOST` to match `rustc`'s sysroot,
// whereas Native compilation wants to use a custom `lib/rustlib/$HOST`. If each
// mode has its own sysroot then we avoid sharing that directory and thus file
// locking it.
pub enum CompilationMode {
    Cross(Target),
    Native(String),
}

impl CompilationMode {
    pub fn hash<H>(&self, hasher: &mut H) -> Result<()>
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
    pub fn triple(&self) -> &str {
        match *self {
            CompilationMode::Cross(ref target) => target.triple(),
            CompilationMode::Native(ref triple) => triple,
        }
    }

    /// Returns the original target triple passed to xargo (perhaps with `.json` extension).
    pub fn orig_triple(&self) -> &str {
        match *self {
            CompilationMode::Cross(ref target) => target.orig_triple(),
            CompilationMode::Native(ref triple) => triple,
        }
    }

    pub fn is_native(&self) -> bool {
        match *self {
            CompilationMode::Native(_) => true,
            _ => false,
        }
    }
}