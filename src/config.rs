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
    pub fn from_metadata(metadata: &cargo_metadata::Metadata) -> Result<Config> {
        let root_package_id = metadata
            .resolve
            .as_ref()
            .and_then(|resolve| resolve.root.clone())
            .ok_or("Cannot infer the root project id")?;

        // Find the root package by id in the list of packages. It is logical error if the root
        // package is not found in the list.
        let root_package = metadata
            .packages
            .iter()
            .find(|package| package.id == root_package_id)
            .expect("The package is not found in the `cargo metadata` output");

        let crate_metadata = root_package.metadata.get("cargo-xbuild");
        let config = match crate_metadata {
            Some(json) => {
                serde_json::from_value(json.clone())
                    .map_err(|_| "parsing package.metadata.cargo-xbuild section failed")?
            }
            None => ParseConfig::default(),
        };

        Ok(Config {
            memcpy: config.memcpy.unwrap_or(true),
            sysroot_path: PathBuf::from(config.sysroot_path.unwrap_or("target/sysroot".into())),
            panic_immediate_abort: config.panic_immediate_abort.unwrap_or(false),
        })
    }
}
