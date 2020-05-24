use cargo_metadata;
use serde_json;
use std::path::PathBuf;

use errors::*;

#[derive(Debug, Hash)]
pub struct Config {
    pub memcpy: bool,
    pub sysroot_path: PathBuf,
    pub panic_immediate_abort: bool,
}

#[derive(Debug, Deserialize, Default)]
struct ParseConfig {
    pub memcpy: Option<bool>,
    pub sysroot_path: Option<String>,
    pub panic_immediate_abort: Option<bool>,
}

impl Config {
    pub fn from_metadata(metadata: &cargo_metadata::Metadata, quiet: bool) -> Result<Config> {
        let root_manifest = metadata.workspace_root.join("Cargo.toml");
        let root_package = metadata
            .packages
            .iter()
            .find(|package| package.manifest_path == root_manifest);

        let config = match root_package {
            Some(root_package) => {
                let crate_metadata = root_package.metadata.get("cargo-xbuild");
                match crate_metadata {
                    Some(json) => serde_json::from_value(json.clone()).map_err(|e| {
                        format!(
                            "parsing package.metadata.cargo-xbuild section failed: {}",
                            e
                        )
                    })?,
                    None => ParseConfig::default(),
                }
            }
            None => {
                // The project has no root package. This could be because it only defines a
                // dummy `Cargo.toml` at the root without a `[package]` section.
                //
                // The problem in this case is that we don't have a reasonable place to read
                // the config from. There are multiple `Cargo.toml`s in this workspace and it
                // is not clear which one is the "canonical" Cargo.toml with the xbuild config.
                //
                // So we can't read the config for such projects. To make this transparent to
                // the user, we print a warning.
                if !quiet {
                    eprintln!(
                        "WARNING: There is no root package to read the cargo-xbuild config from."
                    );
                }
                // There is no config to read, so we use default options
                ParseConfig::default()
            }
        };

        Ok(Config {
            memcpy: config.memcpy.unwrap_or(true),
            sysroot_path: PathBuf::from(config.sysroot_path.unwrap_or("target/sysroot".into())),
            panic_immediate_abort: config.panic_immediate_abort.unwrap_or(false),
        })
    }
}
