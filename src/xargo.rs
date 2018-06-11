use std::path::{Display, PathBuf};
use std::process::{Command, ExitStatus};
use std::{fs, mem};
use std::io::{self, Write};
use std::path::Path;

use rustc_version::VersionMeta;

use CompilationMode;
use cargo::Rustflags;
use cli::Args;
use errors::*;
use extensions::CommandExt;
use flock::{FileLock, Filesystem};
use config::Config;

pub fn run(
    args: &Args,
    cmode: &CompilationMode,
    rustflags: Rustflags,
    home: &Home,
    meta: &VersionMeta,
    verbose: bool,
) -> Result<ExitStatus> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    cmd.args(args.all());

    let flags = rustflags.for_xargo(home);
    if verbose {
        writeln!(io::stderr(), "+ RUSTFLAGS={:?}", flags).ok();
    }
    cmd.env("RUSTFLAGS", flags);

    let locks = (home.lock_ro(&meta.host), home.lock_ro(cmode.triple()));

    let status = cmd.run_and_get_status(verbose)?;

    mem::drop(locks);

    Ok(status)
}

pub struct Home {
    path: Filesystem,
}

impl Home {
    pub fn display(&self) -> Display {
        self.path.display()
    }

    fn path(&self, triple: &str) -> Filesystem {
        self.path.join("lib/rustlib").join(triple)
    }

    pub fn lock_ro(&self, triple: &str) -> Result<FileLock> {
        let fs = self.path(triple);

        fs.open_ro(".sentinel", &format!("{}'s sysroot", triple))
            .chain_err(|| format!("couldn't lock {}'s sysroot as read-only", triple))
    }

    pub fn lock_rw(&self, triple: &str) -> Result<FileLock> {
        let fs = self.path(triple);

        fs.open_rw(".sentinel", &format!("{}'s sysroot", triple))
            .chain_err(|| format!("couldn't lock {}'s sysroot as read-write", triple))
    }
}

pub fn home(target_directory: &Path, config: &Config) -> Result<Home> {
    let path = if let Some(ref p) = config.sysroot_path {
        let path = Path::new(p);
        fs::create_dir_all(&path).map_err(|_| String::from("Could not create sysroot folder"))?;
        path.canonicalize().map_err(|_| String::from("Invalid sysroot path"))?
    } else {
        let mut p = PathBuf::from(target_directory);
        p.push("sysroot");
        p
    };

    Ok(Home {
        path: Filesystem::new(path),
    })
}
