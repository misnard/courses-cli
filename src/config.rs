use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    /// Picked store per provider name.
    #[serde(default)]
    pub stores: BTreeMap<String, crate::providers::Store>,
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

/// Holds config.json, and each provider's cookie jar and Chrome profile.
pub fn config_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("COURSES_CONFIG_DIR") {
        return Ok(dir.into());
    }
    let base = dirs::config_dir().context("Could not find config directory")?;
    let dir = base.join("courses-cli");
    // ponytail: one-time move from when the tool was carrefour-cli; drop it in a later release.
    let old = base.join("carrefour-cli");
    if !dir.exists() && old.exists() {
        fs::rename(&old, &dir).with_context(|| format!("Failed to move {:?} to {:?}", old, dir))?;
        let _ = fs::rename(dir.join("cookies.json"), dir.join("carrefour-cookies.json"));
        let _ = fs::rename(dir.join("browser-profile"), dir.join("carrefour-profile"));
    }
    Ok(dir)
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn show_config() -> Result<()> {
    let config = Config::load()?;
    println!("Config file: {:?}", config_path()?);
    println!();

    let jar = crate::session::load_jar()?;
    println!(
        "Carrefour cookies: {:?} ({} cookies, SSO: {})",
        crate::session::jar_path()?,
        jar.len(),
        if crate::session::has_sso(&jar) { "yes" } else { "no" }
    );

    for (provider, s) in &config.stores {
        println!("{} store: {} ({}) {}", provider, s.name, s.id, s.info);
    }
    Ok(())
}
