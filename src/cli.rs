use std::env;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

pub struct Args {
    all: Vec<String>,
    target: Option<String>,
    manifest_path: Option<PathBuf>,
    verbosity: Option<Verbosity>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Verbosity {
    Quiet,
    Verbose,
}

impl Args {
    /// Create args explicitly, with other args passed unchanged to cargo invocation
    pub fn new<T, P, A, S>(
        target: Option<T>,
        manifest_path: Option<P>,
        verbosity: Option<Verbosity>,
        other_args: A,
    ) -> Result<Self>
    where
        T: Into<String> + Clone,
        P: AsRef<Path>,
        A: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let other_args = other_args
            .into_iter()
            .map(|a| a.as_ref().to_string())
            .collect::<Vec<_>>();

        // check for duplicates of the explicit args
        let explicit_args = ["--target", "--manifest-path", "--verbose", "--quiet"];
        let duplicates = other_args
            .iter()
            .filter(|a| {
                explicit_args
                    .iter()
                    .any(|ea| a == ea || a.starts_with(&format!("{}=", ea)))
            })
            .collect::<Vec<_>>();
        if !duplicates.is_empty() {
            return Err(anyhow!(
                "The following args should be passed explicitly: {:?}",
                duplicates
            ));
        }

        // add the explicit args to `all` which will be passed on to `cargo`
        let mut all = other_args;
        if let Some(target) = target.clone() {
            all.push(format!("--target={}", target.into()))
        }
        if let Some(ref manifest_path) = manifest_path {
            all.push(format!(
                "--manifest-path={}",
                manifest_path.as_ref().to_string_lossy()
            ))
        }
        if let Some(ref verbosity) = verbosity {
            match verbosity {
                Verbosity::Verbose => all.push("--verbose".into()),
                Verbosity::Quiet => all.push("--quiet".into()),
            }
        }

        Ok(Args {
            all,
            target: target.map(Into::into),
            manifest_path: manifest_path.map(|p| p.as_ref().into()),
            verbosity,
        })
    }

    /// Parse raw args from command line
    pub fn from_raw<A, S>(all: A) -> Result<Self>
    where
        A: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let all = all
            .into_iter()
            .map(|a| a.as_ref().to_string())
            .collect::<Vec<_>>();

        let mut target: Option<String> = None;
        let mut manifest_path = None;
        let mut verbosity = None;
        {
            let mut args = all.iter();
            while let Some(arg) = args.next() {
                if arg == "--target" {
                    target = args.next().map(|s| s.to_owned());
                } else if arg.starts_with("--target=") {
                    target = arg.splitn(2, '=').nth(1).map(|s| s.to_owned());
                }
                if arg == "--manifest-path" {
                    manifest_path = args.next().map(|s| s.to_owned());
                } else if arg.starts_with("--manifest-path=") {
                    manifest_path = arg.splitn(2, '=').nth(1).map(|s| s.to_owned());
                }
                if arg == "--verbose" || arg == "-v" || arg == "-vv" {
                    if let Some(Verbosity::Quiet) = verbosity {
                        return Err(anyhow!("cannot set both --verbose and --quiet"));
                    }
                    verbosity = Some(Verbosity::Verbose)
                }
                if arg == "--quiet" || arg == "-q" {
                    if let Some(Verbosity::Verbose) = verbosity {
                        return Err(anyhow!("cannot set both --verbose and --quiet"));
                    }
                    verbosity = Some(Verbosity::Quiet)
                }
            }
        }

        Ok(Args {
            all,
            target: target.map(Into::into),
            manifest_path: manifest_path.map(Into::into),
            verbosity,
        })
    }

    pub fn all(&self) -> &[String] {
        &self.all
    }

    pub fn target(&self) -> Option<&str> {
        self.target.as_ref().map(|s| &**s)
    }

    pub fn manifest_path(&self) -> Option<&Path> {
        self.manifest_path.as_ref().map(|s| &**s)
    }

    pub fn quiet(&self) -> bool {
        self.verbosity == Some(Verbosity::Quiet)
    }

    pub fn verbose(&self) -> bool {
        self.verbosity == Some(Verbosity::Verbose)
    }
}

pub fn args(command_name: &str) -> Result<(Command, Args)> {
    let mut args = env::args().skip(1);
    if args.next() != Some("x".to_string() + command_name) {
        Err(anyhow!(
            "must be invoked as cargo subcommand: `cargo x{}`",
            command_name
        ))?;
    }
    let all = args.collect::<Vec<_>>();
    let command = match all.first().map(|s| s.as_str()) {
        Some("-h") | Some("--help") => Command::Help,
        Some("-v") | Some("--version") => Command::Version,
        _ => Command::Build,
    };

    let args = Args::from_raw(all)?;
    Ok((command, args))
}

#[derive(Clone, PartialEq)]
pub enum Command {
    Build,
    Help,
    Version,
}
