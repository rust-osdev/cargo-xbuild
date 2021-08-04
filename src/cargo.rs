use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::{env, fmt};

use anyhow::{anyhow, Context, Result};
use toml::Value;

use crate::cli::Args;
use crate::extensions::CommandExt;
use crate::util;
use crate::xargo::Home;

pub struct Rustflags {
    flags: Vec<String>,
}

impl Rustflags {
    pub fn hash<H>(&self, hasher: &mut H)
    where
        H: Hasher,
    {
        let mut flags = self.flags.iter();

        while let Some(flag) = flags.next() {
            if flag == "-C" {
                if let Some(next) = flags.next() {
                    if next.starts_with("link-arg=") || next.starts_with("link-args=") {
                        // don't hash linker arguments
                    } else {
                        flag.hash(hasher);
                        next.hash(hasher);
                    }
                } else {
                    flag.hash(hasher);
                }
            } else {
                flag.hash(hasher);
            }
        }
    }

    /// Stringifies these flags for Xargo consumption
    pub fn for_xargo(&self, home: &Home) -> Result<String> {
        let sysroot = format!("{}", home.display());
        if env::var_os("XBUILD_ALLOW_SYSROOT_SPACES").is_none() && sysroot.contains(" ") {
            return Err(anyhow!(
                "Sysroot must not contain spaces!\n\
                See issue https://github.com/rust-lang/cargo/issues/6139\n\n\
                The sysroot is `{}`.\n\n\
                To override this error, you can set the `XBUILD_ALLOW_SYSROOT_SPACES`\
                environment variable.",
                sysroot
            ));
        }
        let mut flags = self.flags.clone();
        if !flags.contains(&String::from("--sysroot")) {
            flags.push("--sysroot".to_owned());
            flags.push(sysroot);
        }
        Ok(flags.join(" "))
    }
}

impl fmt::Display for Rustflags {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.flags.join(" "), f)
    }
}

pub fn rustflags(config: Option<&Config>, target: &str) -> Result<Rustflags> {
    flags(config, target, "rustflags").map(|fs| Rustflags { flags: fs })
}

/// Returns the flags for `tool` (e.g. rustflags)
///
/// This looks into the environment and into `.cargo/config`
fn flags(config: Option<&Config>, target: &str, tool: &str) -> Result<Vec<String>> {
    if let Some(t) = env::var_os(tool.to_uppercase()) {
        return Ok(t
            .to_string_lossy()
            .split_whitespace()
            .map(|w| w.to_owned())
            .collect());
    }

    if let Some(config) = config.as_ref() {
        let mut build = false;
        if let Some(array) = config
            .table
            .get("target")
            .and_then(|v| v.get(target))
            .and_then(|v| v.get(tool))
            .or_else(|| {
                build = true;
                config.table.get("build").and_then(|v| v.get(tool))
            })
        {
            let mut flags = vec![];

            let mut error = false;
            if let Value::Array(array) = array {
                for value in array {
                    if let Some(flag) = value.as_str() {
                        flags.push(flag.to_owned());
                    } else {
                        error = true;
                        break;
                    }
                }
            } else {
                error = true;
            }

            if error {
                if build {
                    Err(anyhow!(
                        ".cargo/config: build.{} must be an array \
                         of strings",
                        tool
                    ))?
                } else {
                    Err(anyhow!(
                        ".cargo/config: target.{}.{} must be an \
                         array of strings",
                        target, tool
                    ))?
                }
            } else {
                Ok(flags)
            }
        } else {
            Ok(vec![])
        }
    } else {
        Ok(vec![])
    }
}

pub fn run(args: &Args, verbose: bool) -> Result<ExitStatus> {
    let cargo = std::env::var("CARGO").unwrap_or("cargo".to_string());
    Command::new(cargo)
        .arg("build")
        .args(args.all())
        .run_and_get_status(verbose)
}

#[derive(Debug)]
pub struct Config {
    parent_path: PathBuf,
    table: Value,
}

impl Config {
    pub fn target(&self) -> Result<Option<String>> {
        if let Some(v) = self.table.get("build").and_then(|v| v.get("target")) {
            let target = v
                .as_str()
                .ok_or_else(|| anyhow!(".cargo/config: build.target must be a string"))?;
            if target.ends_with(".json") {
                let target_path = self.parent_path.join(target);
                let canonicalized = target_path.canonicalize().map_err(|err| {
                    anyhow!(
                        "target JSON file {} does not exist: {}",
                        target_path.display(),
                        err
                    )
                })?;
                let as_string = canonicalized
                    .into_os_string()
                    .into_string()
                    .map_err(|err| anyhow!("target path not valid utf8: {:?}", err))?;
                Ok(Some(as_string))
            } else {
                Ok(Some(target.to_owned()))
            }
        } else {
            Ok(None)
        }
    }
}

pub fn config() -> Result<Option<Config>> {
    let cd = env::current_dir().with_context(|| "couldn't get the current directory")?;

    if let Some(p) = util::search(&cd, ".cargo/config") {
        Ok(Some(Config {
            parent_path: p.to_owned(),
            table: util::parse(&p.join(".cargo/config"))?,
        }))
    } else {
        Ok(None)
    }
}

pub struct Profile<'t> {
    table: &'t Value,
}

impl<'t> Profile<'t> {
    pub fn hash<H>(&self, hasher: &mut H)
    where
        H: Hasher,
    {
        let mut v = self.table.clone();

        // Don't include `lto` in the hash because it doesn't affect compilation
        // of `.rlib`s
        if let Value::Table(ref mut table) = v {
            table.remove("lto");

            // don't hash an empty map
            if table.is_empty() {
                return;
            }
        }

        v.to_string().hash(hasher);
    }
}

impl<'t> fmt::Display for Profile<'t> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut map = toml::map::Map::new();
        map.insert("profile".to_owned(), {
            let mut map = toml::map::Map::new();
            map.insert("release".to_owned(), self.table.clone());
            Value::Table(map)
        });

        fmt::Display::fmt(&Value::Table(map), f)
    }
}

pub struct Features<'a> {
    array: &'a Value,
}

impl<'a> fmt::Display for Features<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut map = toml::map::Map::new();
        map.insert("cargo-features".to_owned(), self.array.clone());

        fmt::Display::fmt(&Value::Table(map), f)
    }
}

pub struct Toml {
    table: Value,
}

impl Toml {
    /// `profile.release` part of `Cargo.toml`
    pub fn profile(&self) -> Option<Profile> {
        self.table
            .get("profile")
            .and_then(|v| v.get("release"))
            .map(|t| Profile { table: t })
    }

    pub fn features(&self) -> Option<Features> {
        self.table
            .get("cargo-features")
            .map(|a| Features { array: a })
    }
}

pub fn toml(root: &Path) -> Result<Toml> {
    util::parse(&root.join("Cargo.toml")).map(|t| Toml { table: t })
}
