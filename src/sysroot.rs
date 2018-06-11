use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;
use std::env;

use rustc_version::VersionMeta;
use tempdir::TempDir;
use toml::{Table, Value};

use CompilationMode;
use cargo::Rustflags;
use errors::*;
use extensions::CommandExt;
use rustc::{Src, Sysroot, Target};
use util;
use xargo::Home;
use cargo;
use config::Config;

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

    build_libcore(cmode, &ctoml, home, src, &dst, verbose)?;
    build_libcompiler_builtins(cmode, &ctoml, home, src, &dst, config, verbose)?;
    build_liballoc(cmode, &ctoml, home, src, &dst, verbose)?;

    // Create hash file
    util::write(&rustlib.parent().join(".hash"), &hash.to_string())?;

    Ok(())
}

fn build_crate(
    crate_name: &str,
    mut stoml: String,
    cmode: &CompilationMode,
    ctoml: &cargo::Toml,
    home: &Home,
    dst: &Path,
    verbose: bool,
) -> Result<()> {
    let td = TempDir::new("xargo").chain_err(|| "couldn't create a temporary directory")?;
    let td = td.path();

    if let Some(profile) = ctoml.profile() {
        stoml.push_str(&profile.to_string())
    }

    util::write(&td.join("Cargo.toml"), &stoml)?;
    util::mkdir(&td.join("src"))?;
    util::write(&td.join("src/lib.rs"), "")?;

    let mut cmd = Command::new("cargo");
    cmd.env_remove("CARGO_TARGET_DIR");
    cmd.env_remove("RUSTFLAGS");
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
    cmd.arg("--sysroot");
    cmd.arg(home.display().to_string());
    cmd.arg("-Z");
    cmd.arg("force-unstable-if-unmarked");

    cmd.run(verbose)?;

    // Copy artifacts to Xargo sysroot
    util::cp_r(
        &td.join("target")
            .join(cmode.triple())
            .join(profile())
            .join("deps"),
        dst,
    )?;

    Ok(())
}

fn build_libcore(
    cmode: &CompilationMode,
    ctoml: &cargo::Toml,
    home: &Home,
    src: &Src,
    dst: &Path,
    verbose: bool,
) -> Result<()> {
    const TOML: &'static str = r#"
[package]
authors = ["The Rust Project Developers"]
name = "sysroot"
version = "0.0.0"
"#;

    let mut stoml = TOML.to_owned();

    let path = src.path().join("libcore").display().to_string();
    let mut core_dep = Table::new();
    core_dep.insert("path".to_owned(), Value::String(path));
    let mut deps = Table::new();
    deps.insert("core".to_owned(), Value::Table(core_dep));
    let mut map = Table::new();
    map.insert("dependencies".to_owned(), Value::Table(deps));
    stoml.push_str(&Value::Table(map).to_string());

    build_crate("core", stoml, cmode, ctoml, home, dst, verbose)
}

fn build_libcompiler_builtins(
    cmode: &CompilationMode,
    ctoml: &cargo::Toml,
    home: &Home,
    src: &Src,
    dst: &Path,
    config: &Config,
    verbose: bool,
) -> Result<()> {
    const TOML: &'static str = r#"
[package]
authors = ["The Rust Project Developers"]
name = "sysroot"
version = "0.0.0"
"#;

    let mut stoml = TOML.to_owned();

    let path = src.path()
        .join("libcompiler_builtins")
        .display()
        .to_string();
    let mut compiler_builtin_dep = Table::new();
    compiler_builtin_dep.insert("path".to_owned(), Value::String(path));

    let mut features = vec![
            Value::String("compiler-builtins".to_owned()),
        ];
    if config.memcpy {
        features.push(Value::String("mem".to_owned()));
    }
    compiler_builtin_dep.insert("default-features".to_owned(), Value::Boolean(false));
    compiler_builtin_dep.insert(
        "features".to_owned(),
        Value::Array(features),
    );
    let mut deps = Table::new();
    deps.insert(
        "compiler_builtins".to_owned(),
        Value::Table(compiler_builtin_dep),
    );
    let mut map = Table::new();
    map.insert("dependencies".to_owned(), Value::Table(deps));
    stoml.push_str(&Value::Table(map).to_string());

    build_crate("compiler_builtins", stoml, cmode, ctoml, home, dst, verbose)
}

fn build_liballoc(
    cmode: &CompilationMode,
    ctoml: &cargo::Toml,
    home: &Home,
    src: &Src,
    dst: &Path,
    verbose: bool,
) -> Result<()> {
    const TOML: &'static str = r#"
[package]
authors = ["The Rust Project Developers"]
name = "alloc"
version = "0.0.0"
"#;

    let mut stoml = TOML.to_owned();

    let path = src.path().join("liballoc/lib.rs").display().to_string();
    let mut map = Table::new();
    let mut lib = Table::new();
    lib.insert("name".to_owned(), Value::String("alloc".to_owned()));
    lib.insert("path".to_owned(), Value::String(path));
    map.insert("lib".to_owned(), Value::Table(lib));
    stoml.push_str(&Value::Table(map).to_string());

    build_crate("alloc", stoml, cmode, ctoml, home, dst, verbose)
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
    util::cp_r(
        &sysroot
            .path()
            .join("lib/rustlib")
            .join(&meta.host)
            .join("lib"),
        &dst,
    )?;

    let bin_dst = lock.parent().join("bin");
    util::mkdir(&bin_dst)?;
    util::cp_r(
        &sysroot
            .path()
            .join("lib/rustlib")
            .join(&meta.host)
            .join("bin"),
        &bin_dst,
    )?;

    util::write(&hfile, hash)?;

    Ok(())
}
