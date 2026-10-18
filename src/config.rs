use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub store_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leclerc_store: Option<crate::leclerc::Store>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read config from {:?}", path))?;
            serde_json::from_str(&content).context("Failed to parse config")
        } else {
            Ok(Config::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("Failed to write config to {:?}", path))
    }
}

/// Holds config.json, the cookie jar and the headless Chrome profile.
pub fn config_dir() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().context("Could not find config directory")?;
    Ok(config_dir.join("carrefour-cli"))
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn show_config() -> Result<()> {
    let config = Config::load()?;
    let path = config_path()?;

    println!("Config file: {:?}", path);
    println!();

    let jar = crate::session::load_jar()?;
    println!(
        "Cookies: {:?} ({} cookies, SSO: {})",
        crate::session::jar_path()?,
        jar.len(),
        if crate::session::has_sso(&jar) { "yes" } else { "no" }
    );

    if let Some(store_id) = &config.store_id {
        println!("Store ID: {}", store_id);
    } else {
        println!("Store ID: not set");
    }

    if let Some(s) = &config.leclerc_store {
        println!("Leclerc store: {} ({}, {})", s.name, s.id, s.host);
    }

    Ok(())
}
