use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    /// FRONTONE_SESSID cookie value from browser session
    pub session_id: Option<String>,
    /// Full cookie string (for Cloudflare bypass)
    pub cookies: Option<String>,
    /// User agent to use (from FlareSolverr)
    pub user_agent: Option<String>,
    pub store_id: Option<String>,
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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content).with_context(|| format!("Failed to write config to {:?}", path))
    }
}

fn config_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().context("Could not find config directory")?;
    Ok(config_dir.join("carrefour-cli").join("config.json"))
}

pub fn show_config() -> Result<()> {
    let config = Config::load()?;
    let path = config_path()?;

    println!("Config file: {:?}", path);
    println!();

    if let Some(cookies) = &config.cookies {
        let masked = if cookies.len() > 30 {
            format!("{}...({} chars)", &cookies[..20], cookies.len())
        } else {
            "[set]".to_string()
        };
        println!("Cookies: {}", masked);
    } else if let Some(session_id) = &config.session_id {
        let masked = if session_id.len() > 10 {
            format!(
                "{}...{}",
                &session_id[..5],
                &session_id[session_id.len() - 5..]
            )
        } else {
            "[set]".to_string()
        };
        println!("Session ID (FRONTONE_SESSID): {}", masked);
    } else {
        println!("Session/Cookies: not set");
    }

    if let Some(store_id) = &config.store_id {
        println!("Store ID: {}", store_id);
    } else {
        println!("Store ID: not set");
    }

    Ok(())
}
