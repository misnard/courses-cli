//! One module per store chain. Each implements `Shop`, and the generic
//! commands (`commands/shop.rs`) print what it returns. A provider only
//! implements what its site allows: the defaults answer "not supported".

mod auchan;
mod carrefour;
mod casino;
mod chronodrive;
mod intermarche;
mod leclerc;
mod superu;

use crate::browser::Browser;
use crate::config::{config_dir, Config};
use crate::session::{read_jar, save_jar_to};
use crate::spec::{Body, Request};
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug, ValueEnum)]
pub enum Provider {
    Carrefour,
    Leclerc,
    Chronodrive,
    Intermarche,
    Auchan,
    Superu,
    Casino,
}

impl Provider {
    /// Key for the config, profile and cookie files, and for messages.
    pub fn name(self) -> &'static str {
        match self {
            Provider::Carrefour => "carrefour",
            Provider::Leclerc => "leclerc",
            Provider::Chronodrive => "chronodrive",
            Provider::Intermarche => "intermarche",
            Provider::Auchan => "auchan",
            Provider::Superu => "superu",
            Provider::Casino => "casino",
        }
    }

    /// `courses` prefix that selects this provider, for hints in messages.
    pub fn cmd(self) -> String {
        match self {
            Provider::Carrefour => "courses".into(),
            p => format!("courses --provider {}", p.name()),
        }
    }
}

/// `headless: false` opens a window, for `auth login`.
pub fn open(p: Provider, headless: bool) -> Result<Box<dyn Shop>> {
    Ok(match p {
        Provider::Carrefour => Box::new(carrefour::Carrefour::open()?),
        Provider::Leclerc => Box::new(leclerc::Leclerc::open(headless)?),
        Provider::Chronodrive => Box::new(chronodrive::Chronodrive::open(headless)?),
        Provider::Intermarche => Box::new(intermarche::Intermarche::open(headless)?),
        Provider::Auchan => Box::new(auchan::Auchan::open(headless)?),
        Provider::Superu => Box::new(superu::Superu::open(headless)?),
        Provider::Casino => Box::new(casino::Casino::open(headless)?),
    })
}

#[derive(Debug, Default, Serialize)]
pub struct Product {
    /// What `cart add` takes: the EAN at Carrefour, the site's own id elsewhere.
    pub id: String,
    pub name: String,
    pub brand: Option<String>,
    pub price: Option<f64>,
    /// As the site words it, e.g. "0,99 € / l"
    pub unit_price: Option<String>,
    pub packaging: Option<String>,
    pub nutriscore: Option<String>,
    pub ean: Option<String>,
    pub available: bool,
    /// Quantity already in the cart, 0 when the site does not say.
    pub in_cart: u32,
}

#[derive(Debug, Default, Serialize)]
pub struct CartLine {
    pub product: Product,
    pub quantity: u32,
    pub total: Option<f64>,
}

#[derive(Debug, Default, Serialize)]
pub struct Cart {
    pub lines: Vec<CartLine>,
    pub total: Option<f64>,
    /// Pickup slot or next available pickup, shown under the total.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    pub id: String,
    pub name: String,
    /// Shown in `store` lists: service type, distance, town...
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub info: String,
    /// Whatever else the provider needs to shop there (host, region...).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

fn unsupported<T>(what: &str) -> Result<T> {
    bail!("{what} is not supported for this provider yet.")
}

pub trait Shop {
    /// Up to `limit` products; `limit` is a hint, the caller truncates.
    fn search(&mut self, _query: &str, _limit: u32) -> Result<Vec<Product>> {
        unsupported("Search")
    }
    fn cart(&mut self) -> Result<Cart> {
        unsupported("The cart")
    }
    /// Set a product's quantity, 0 removing it. Returns the quantity the cart
    /// really holds afterwards (0 for an unknown id, less when stock is short).
    fn set_quantity(&mut self, _id: &str, _quantity: u32) -> Result<u32> {
        unsupported("Changing the cart")
    }
    /// Stores near a postal code or city, nearest first.
    fn find_stores(&mut self, _query: &str) -> Result<Vec<Store>> {
        unsupported("Finding stores")
    }
    /// Let the user sign in, in the window the shop was opened with.
    fn login(&mut self) -> Result<()> {
        unsupported("Signing in")
    }
    fn signed_in(&mut self) -> Result<bool> {
        unsupported("Checking the session")
    }
}

// --------------------------------------------------------------------- store

pub fn store(p: Provider) -> Result<Store> {
    Config::load()?
        .stores
        .remove(p.name())
        .with_context(|| format!("No {} store picked. Run '{} store <postal code>'.", p.name(), p.cmd()))
}

pub fn save_store(p: Provider, s: Store) -> Result<()> {
    let mut config = Config::load()?;
    config.stores.insert(p.name().into(), s);
    config.save()
}

// ----------------------------------------------------------------------- web

/// A provider's own Chrome: profile `<name>-profile` and cookie jar
/// `<name>-cookies.json`, so two sites never share a session.
pub struct Web {
    pub browser: Browser,
    provider: Provider,
    jar: PathBuf,
    /// Cookies kept across runs: Chrome never writes session cookies to disk.
    domain: &'static str,
}

impl Web {
    pub fn open(provider: Provider, domain: &'static str, headless: bool) -> Result<Web> {
        let dir = config_dir()?;
        let jar = dir.join(format!("{}-cookies.json", provider.name()));
        let profile = dir.join(format!("{}-profile", provider.name()));
        Ok(Web { browser: Browser::launch(&profile, &read_jar(&jar), headless)?, provider, jar, domain })
    }

    /// Request `url` from a page parked on `page` (see `Browser::fetch_from`).
    /// Fails on a non-2xx answer.
    pub fn send(&mut self, page: &str, method: &str, url: &str, headers: &[(&str, &str)], body: Option<String>) -> Result<String> {
        let headers: Map<String, Value> = headers.iter().map(|(k, v)| (k.to_lowercase(), json!(v))).collect();
        let req = Request { method: method.into(), url: url.into(), headers, body: body.map(Body::Text) };
        let res = self.browser.fetch_from(page, &req)?;
        match res.status {
            200..=299 => Ok(res.text),
            403 | 429 => bail!(
                "Blocked by the site's bot protection (HTTP {}); run '{} auth login' to sign in or solve the captcha.",
                res.status,
                self.provider.cmd()
            ),
            s => bail!("HTTP {s} from {url}: {}", res.text.chars().take(200).collect::<String>()),
        }
    }

    /// `send` with a JSON body (when given) and a JSON answer.
    pub fn json(&mut self, page: &str, method: &str, url: &str, headers: &[(&str, &str)], body: Option<Value>) -> Result<Value> {
        let mut h = headers.to_vec();
        h.push(("accept", "application/json"));
        if body.is_some() {
            h.push(("content-type", "application/json"));
        }
        let text = self.send(page, method, url, &h, body.map(|b| b.to_string()))?;
        let text = if text.trim().is_empty() { "null" } else { &text };
        serde_json::from_str(text).with_context(|| format!("Unexpected answer from {url}"))
    }

    /// Show `url` in the window (opened with headless: false) until the user is done.
    pub fn login(&mut self, url: &str) -> Result<()> {
        self.browser.open(url)?;
        println!("Sign in in the Chrome window (and solve any captcha), then press Enter here.");
        std::io::stdin().read_line(&mut String::new())?;
        Ok(())
    }
}

impl Drop for Web {
    fn drop(&mut self) {
        match self.browser.cookies(self.domain) {
            Ok(c) if !c.is_empty() => {
                if let Err(e) = save_jar_to(&self.jar, &c) {
                    eprintln!("Warning: could not save cookies: {e}");
                }
            }
            _ => {}
        }
    }
}

/// Forget a provider's session: its cookie jar and Chrome profile.
pub fn logout(p: Provider) -> Result<()> {
    let dir = config_dir()?;
    let _ = std::fs::remove_file(dir.join(format!("{}-cookies.json", p.name())));
    let _ = std::fs::remove_dir_all(dir.join(format!("{}-profile", p.name())));
    Ok(())
}

// ------------------------------------------------------------------- helpers

/// A JSON string, or a number as text (ids come as either).
pub fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// "11,88 €" → 11.88
pub fn parse_euros(s: &str) -> Option<f64> {
    let n: String = s.chars().filter(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '-')).collect();
    n.replace(',', ".").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(parse_euros("11,88 €"), Some(11.88));
        assert_eq!(parse_euros("n/a"), None);
        assert_eq!(str_of(&json!(53701)), Some("53701".into()));
        assert_eq!(str_of(&json!("053701")), Some("053701".into()));
    }
}
