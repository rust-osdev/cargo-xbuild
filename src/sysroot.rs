use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;

use rustc_version::VersionMeta;
use tempfile::Builder;
use toml::{value::Table, Value};

use cargo;
use cargo::Rustflags;
use config::Config;
use errors::*;
use extensions::CommandExt;
use rustc::{Src, Sysroot, Target};
use util;
use xargo::Home;
use CompilationMode;

#[cfg(feature = "dev")]
fn profile() -> &'static str {
    "debug"
}

#[cfg(not(feature = "dev"))]
fn profile() -> &'static str {
    "release"
}

fn build(
    cmode: &CompilationMode,
    ctoml: &cargo::Toml,
    home: &Home,
    config: &Config,
    src: &Src,
    hash: u64,
    verbose: bool,
) -> Result<()> {
    let rustlib = home.lock_rw(cmode.triple())?;
    rustlib
        .remove_siblings()
        .chain_err(|| format!("couldn't clear {}", rustlib.path().display()))?;
    let dst = rustlib.parent().join("lib");
    util::mkdir(&dst)?;

    build_liballoc(cmode, &ctoml, src, &dst, config, verbose)?;

    // Create hash file
    util::write(&rustlib.parent().join(".hash"), &hash.to_string())?;

    Ok(())
}

fn build_crate(
    crate_name: &str,
    lockfile: &Path,
    mut stoml: String,
    cmode: &CompilationMode,
    ctoml: &cargo::Toml,
    dst: &Path,
    verbose: bool,
) -> Result<()> {
    let td = Builder::new()
        .prefix("cargo-xbuild")
        .tempdir()
        .chain_err(|| "couldn't create a temporary directory")?;
    let td_path;
    let td = if env::var_os("XBUILD_KEEP_TEMP").is_some() {
        td_path = td.into_path();
        println!("XBUILD_KEEP_TEMP: files at {:?}", td_path);
        &td_path
    } else {
        td.path()
    };

    let target_dir = td.join("target");

    if let Some(features) = ctoml.features() {
        stoml.insert_str(0, &features.to_string())
    }

    if let Some(profile) = ctoml.profile() {
        stoml.push_str(&profile.to_string())
    }

    util::write(&td.join("Cargo.toml"), &stoml)?;
    fs::copy(lockfile, &td.join("Cargo.lock")).chain_err(|| {
        format!(
            "failed to copy Cargo.lock from `{}` to `{}`",
            lockfile.display(),
            &td.join("Cargo.lock").display()
        )
    })?;
    util::mkdir(&td.join("src"))?;
    util::write(&td.join("src/lib.rs"), "")?;

    let cargo = std::env::var("CARGO").unwrap_or("cargo".to_string());
    let mut cmd = Command::new(cargo);
    cmd.env("RUSTFLAGS", "-Cembed-bitcode=yes");
    cmd.env("CARGO_TARGET_DIR", &target_dir);
    cmd.env("__CARGO_DEFAULT_LIB_METADATA", "XARGO");

    // As of rust-lang/cargo#4788 Cargo invokes rustc with a changed "current directory" so
    // we can't assume that such directory will be the same as the directory from which
    // Xargo was invoked. This is specially true when compiling the sysroot as the std
    // source is provided as a workspace and Cargo will change the current directory to the
    // root of the workspace when building one. To ensure rustc finds a target specification
    // file stored in the current directory we'll set `RUST_TARGET_PATH`  to the current
    // directory.
    if env::var_os("RUST_TARGET_PATH").is_none() {
        if let CompilationMode::Cross(ref target) = *cmode {
            if let Target::Custom { ref json, .. } = *target {
                cmd.env("RUST_TARGET_PATH", json.parent().unwrap());
            }
        }
    }

    cmd.arg("rustc");
    cmd.arg("-p").arg(crate_name);

    match () {
        #[cfg(feature = "dev")]
        () => {}
        #[cfg(not(feature = "dev"))]
        () => {
            cmd.arg("--release");
        }
    }
    cmd.arg("--manifest-path");
    cmd.arg(td.join("Cargo.toml"));
    cmd.args(&["--target", cmode.orig_triple()]);

    if verbose {
        cmd.arg("-v");
    }

    cmd.arg("--");
    cmd.arg("-Z");
    cmd.arg("force-unstable-if-unmarked");

    cmd.run(verbose)?;

    // Copy artifacts to Xargo sysroot
    util::cp_r(
        &target_dir.join(cmode.triple()).join(profile()).join("deps"),
        dst,
    )?;

    Ok(())
}

fn build_liballoc(
    cmode: &CompilationMode,
    ctoml: &cargo::Toml,
    src: &Src,
    dst: &Path,
    config: &Config,
    verbose: bool,
) -> Result<()> {
    const TOML: &'static str = r#"
[package]
authors = ["The Rust Project Developers"]
name = "alloc"
version = "0.0.0"
edition = "2018"

[dependencies.compiler_builtins]
version = "0.1.0"
"#;

    let mut stoml = TOML.to_owned();

    if config.memcpy {
        stoml.push_str("features = ['mem', 'core']\n");
    } else {
        stoml.push_str("features = ['core']\n");
    }

    stoml.push_str("[dependencies.core]\n");
    stoml.push_str(&format!(
        "path = '{}'\n",
        src.path().join("libcore").display()
    ));

    if config.panic_immediate_abort {
        stoml.push_str("features = ['panic_immediate_abort']\n");
    }

    stoml.push_str("[patch.crates-io.rustc-std-workspace-core]\n");
    stoml.push_str(&format!(
        "path = '{}'\n",
        src.path().join("tools/rustc-std-workspace-core").display()
    ));

    let path = src.path().join("liballoc/lib.rs").display().to_string();
    let mut map = Table::new();
    let mut lib = Table::new();
    lib.insert("name".to_owned(), Value::String("alloc".to_owned()));
    lib.insert("path".to_owned(), Value::String(path));
    map.insert("lib".to_owned(), Value::Table(lib));
    stoml.push_str(&Value::Table(map).to_string());

    let lockfile = src.path().join("..").join("Cargo.lock");

    build_crate("alloc", &lockfile, stoml, cmode, ctoml, dst, verbose)
}

fn old_hash(cmode: &CompilationMode, home: &Home) -> Result<Option<u64>> {
    // FIXME this should be `lock_ro`
    let lock = home.lock_rw(cmode.triple())?;
    let hfile = lock.parent().join(".hash");

    if hfile.exists() {
        Ok(util::read(&hfile)?.parse().ok())
    } else {
        Ok(None)
    }
}

/// Computes the hash of the would-be target sysroot
///
/// This information is used to compute the hash
///
/// - RUSTFLAGS / build.rustflags / target.*.rustflags
/// - The target specification file, is any
/// - `[profile.release]` in `Cargo.toml`
/// - `rustc` commit hash
fn hash(
    cmode: &CompilationMode,
    rustflags: &Rustflags,
    ctoml: &cargo::Toml,
    meta: &VersionMeta,
    config: &Config,
) -> Result<u64> {
    let mut hasher = DefaultHasher::new();

    rustflags.hash(&mut hasher);

    cmode.hash(&mut hasher)?;

    if let Some(profile) = ctoml.profile() {
        profile.hash(&mut hasher);
    }

    if let Some(ref hash) = meta.commit_hash {
        hash.hash(&mut hasher);
    }

    config.hash(&mut hasher);

    Ok(hasher.finish())
}

pub fn update(
    cmode: &CompilationMode,
    home: &Home,
    root: &Path,
    config: &Config,
    rustflags: &Rustflags,
    meta: &VersionMeta,
    src: &Src,
    sysroot: &Sysroot,
    verbose: bool,
) -> Result<()> {
    let ctoml = cargo::toml(root)?;

    let hash = hash(cmode, rustflags, &ctoml, meta, config)?;

    if old_hash(cmode, home)? != Some(hash) {
        build(cmode, &ctoml, home, config, src, hash, verbose)?;
    }

    // copy host artifacts into the sysroot, if necessary
    if cmode.is_native() {
        return Ok(());
    }

    let lock = home.lock_rw(&meta.host)?;
    let hfile = lock.parent().join(".hash");

    let hash = meta.commit_hash.as_ref().map(|s| &**s).unwrap_or("");
    if hfile.exists() {
        if util::read(&hfile)? == hash {
            return Ok(());
        }
    }

    lock.remove_siblings()
        .chain_err(|| format!("couldn't clear {}", lock.path().display()))?;
    let dst = lock.parent().join("lib");
    util::mkdir(&dst)?;
    match util::cp_r(
        &sysroot
            .path()
            .join("lib")
            .join("rustlib")
            .join(&meta.host)
            .join("lib"),
        &dst,
    ) {
        Ok(()) => {}
        Err(e) => eprintln!("Unable to copy the directory 'lib' from sysroot: {}", e),
    };

    let bin_dst = lock.parent().join("bin");
    util::mkdir(&bin_dst)?;
    match util::cp_r(
        &sysroot
            .path()
            .join("lib")
            .join("rustlib")
            .join(&meta.host)
            .join("bin"),
        &bin_dst,
    ) {
        Ok(()) => {}
        Err(e) => eprintln!("Unable to copy the directory 'bin' from sysroot: {}", e),
    };

    util::write(&hfile, hash)?;

    Ok(())
}
