use cargo_metadata;
use serde_json;
use std::path::PathBuf;

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
    pub fn from_metadata(metadata: &cargo_metadata::Metadata) -> Result<Config, serde_json::Error> {
        let package_metadata = metadata.packages.first().map(|p| &p.metadata);
        let crate_metadata = package_metadata
            .as_ref()
            .and_then(|m| m.get("cargo-xbuild"));
        let config = match crate_metadata {
            Some(json) => serde_json::from_value(json.clone())?,
            None => ParseConfig::default(),
        };

        Ok(Config {
            memcpy: config.memcpy.unwrap_or(true),
            sysroot_path: PathBuf::from(config.sysroot_path.unwrap_or("target/sysroot".into())),
            panic_immediate_abort: config.panic_immediate_abort.unwrap_or(false),
        })
    }
}
