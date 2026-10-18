//! E.Leclerc Drive provider. Endpoints from
//! https://github.com/skunkobi/mcp-leclerc-drive (MIT), see its docs/api-capture.md.
//!
//! DataDome guards every leclercdrive.fr host; the headless Chrome of
//! `browser.rs` gets through it as it does Cloudflare. Store pages are ASP.NET
//! HTML with the product records and the cart embedded as JSON objects; every
//! cart write is one form POST to `panier.aspx?op=1`.

use crate::browser::Browser;
use crate::config::{config_dir, Config};
use crate::session::{read_jar, save_jar_to};
use crate::spec::{Body, Request};
use anyhow::{bail, Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::PathBuf;

const LOCATOR: &str = "https://api-recherchemagasins.leclercdrive.fr/API_RechercheMagasins/api/v1";
const LOGIN: &str = "run 'carrefour --provider leclerc auth login' to sign in or solve the captcha";
/// Search that matches nothing, so the page only carries the cart's records.
const NO_MATCH: &str = "zzzznomatchzzz";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Store {
    /// noPL, e.g. "053701"
    pub id: String,
    /// noPR: equals `id` for a drive, differs for a pedestrian relay
    pub no_pr: String,
    /// The shopping host varies per store: fd8-, fd9-, fd14-courses.leclercdrive.fr...
    pub host: String,
    pub name: String,
}

impl Store {
    fn base(&self) -> String {
        format!("https://{}/magasin-{}-{}", self.host, self.id, self.no_pr)
    }
}

fn store() -> Result<Store> {
    Config::load()?
        .leclerc_store
        .context("No Leclerc store picked. Run 'carrefour --provider leclerc store <postal code>'.")
}

fn jar_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("leclerc-cookies.json"))
}

/// Own Chrome profile and cookie jar, so Carrefour and Leclerc never mix.
struct Leclerc {
    browser: Browser,
}

impl Leclerc {
    fn open(headless: bool) -> Result<Leclerc> {
        let jar = read_jar(&jar_path()?);
        Ok(Leclerc { browser: Browser::launch(&config_dir()?.join("leclerc-profile"), &jar, headless)? })
    }

    /// `page`: an HTML page carrying DataDome's script, CORS-allowed for `url`.
    fn send(&mut self, page: &str, method: &str, url: &str, form: Option<String>) -> Result<String> {
        let mut headers = Map::new();
        if form.is_some() {
            headers.insert("content-type".into(), json!("application/x-www-form-urlencoded; charset=UTF-8"));
            headers.insert("x-requested-with".into(), json!("XMLHttpRequest"));
        }
        let req = Request { method: method.into(), url: url.into(), headers, body: form.map(Body::Text) };
        let res = self.browser.fetch_from(page, &req)?;
        match res.status {
            200..=299 => Ok(res.text),
            403 | 429 => bail!("Blocked by DataDome (HTTP {}); {LOGIN}.", res.status),
            s => bail!("Leclerc HTTP {s}: {}", res.text.chars().take(200).collect::<String>()),
        }
    }

    fn locate(&mut self, url: &str) -> Result<Value> {
        let text = self.send("https://www.leclercdrive.fr/", "GET", url, None)?;
        serde_json::from_str(&text).with_context(|| format!("Unexpected response from {url}"))
    }

    /// A search page of the store; it also embeds the cart.
    fn page(&mut self, store: &Store, query: &str) -> Result<String> {
        let url = url::Url::parse_with_params(&format!("{}/recherche.aspx", store.base()), [("TexteRecherche", query)])?;
        let html = self.send(&format!("{}/", store.base()), "GET", url.as_str(), None)?;
        // Every real store page carries the cart summary; DataDome's soft
        // challenge and the "session expirée" page (another drive) do not.
        if !html.contains("\"lstProduitsLight\"") {
            if html.contains("session a expir") || html.contains("sessionexpiree") {
                bail!("Leclerc session expired or bound to another drive; {LOGIN}.");
            }
            bail!("Unexpected Leclerc page (DataDome challenge?); {LOGIN}.");
        }
        Ok(html)
    }

    /// (cart lines, cart summary)
    fn cart(&mut self, store: &Store) -> Result<(Vec<Value>, Value)> {
        let html = self.page(store, NO_MATCH)?;
        let lines = records(&html).into_iter().filter(|r| qty(r) > 0).collect();
        let summary = html.find("\"lstProduitsLight\"").and_then(|i| enclosing(&html, i)).unwrap_or_default();
        Ok((lines, summary))
    }

    /// Set a line's quantity. `iQuantite` is the target, but the action must say
    /// which way it goes: 1 adds, 2 removes. Returns the quantity the cart
    /// really holds (0 for an unknown id) and the new cart totals.
    fn set(&mut self, store: &Store, id: &str, quantity: u32) -> Result<(u32, Value)> {
        let (lines, _) = self.cart(store)?;
        let current = lines.iter().find(|r| id_of(r) == id).map(qty).unwrap_or(0);
        let action = if quantity >= current { 1 } else { 2 };
        let d = json!({"eTypeAction": action, "iIdProduit": id, "iQuantite": quantity, "sNoPointLivraison": store.id});
        let form = url::form_urlencoded::Serializer::new(String::new()).append_pair("d", &d.to_string()).finish();
        let text = self.send(&format!("{}/", store.base()), "POST", &format!("{}/panier.aspx?op=1", store.base()), Some(form))?;
        let events: Vec<Value> = serde_json::from_str(&text)
            .with_context(|| format!("Unexpected cart response: {}", text.chars().take(120).collect::<String>()))?;
        let event = |prefix: &str| {
            events.iter().find(|e| e["sIdUnique"].as_str().is_some_and(|s| s.starts_with(prefix))).map(|e| e["objElement"].clone())
        };
        let held = event(&format!("Produit{id}")).map(|p| qty(&p)).unwrap_or(0);
        Ok((held, event("Panier").unwrap_or_default()))
    }

    fn find_stores(&mut self, query: &str) -> Result<Vec<Value>> {
        let q = |path: &str, params: &[(&str, &str)]| url::Url::parse_with_params(&format!("{LOCATOR}{path}"), params);
        let auto = self.locate(q("/autocomplete", &[("search", query), ("provider", "Woosmap")])?.as_str())?;
        let Some(place) = auto["postalCodes"].get(0) else { return Ok(vec![]) };
        let id = place["id"].as_str().unwrap_or("");
        let c = self.locate(q("/autocomplete/coordinates", &[("id", id), ("provider", "Woosmap")])?.as_str())?;
        let (lat, lng) = match (c["latitude"].as_f64(), c["longitude"].as_f64()) {
            (Some(lat), Some(lng)) => (lat, lng),
            _ => (c["coordinates"]["latitude"].as_f64().unwrap_or(0.0), c["coordinates"]["longitude"].as_f64().unwrap_or(0.0)),
        };
        let cp = place["postalCode"].as_str().unwrap_or(query);
        let (lat, lng) = (lat.to_string(), lng.to_string());
        let near = self.locate(q("/MapPoint/nearby", &[("latitude", &lat), ("longitude", &lng), ("postalCode", cp)])?.as_str())?;
        Ok(near["points"].as_array().cloned().unwrap_or_default())
    }
}

impl Drop for Leclerc {
    /// Chrome never writes session cookies to disk: keep them ourselves.
    fn drop(&mut self) {
        match (self.browser.cookies("leclercdrive.fr"), jar_path()) {
            (Ok(c), Ok(path)) if !c.is_empty() => {
                if let Err(e) = save_jar_to(&path, &c) {
                    eprintln!("Warning: could not save cookies: {e}");
                }
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ parsing

/// The smallest JSON object enclosing byte `at`.
// ponytail: the backward walk ignores strings, so a brace inside a label can
// misplace the start; that record then fails to parse and is skipped.
fn enclosing(html: &str, at: usize) -> Option<Value> {
    let mut depth = 0;
    let start = html[..at].bytes().rposition(|b| match b {
        b'}' => {
            depth += 1;
            false
        }
        b'{' if depth == 0 => true,
        b'{' => {
            depth -= 1;
            false
        }
        _ => false,
    })?;
    serde_json::Deserializer::from_str(&html[start..]).into_iter::<Value>().next()?.ok()
}

/// Product records embedded in a page, deduplicated.
fn records(html: &str) -> Vec<Value> {
    let mut seen = HashSet::new();
    html.match_indices("\"iIdProduit\":")
        .filter_map(|(i, _)| enclosing(html, i))
        .filter(|r| r["sType"] == "Produit" && seen.insert(id_of(r)))
        .collect()
}

fn id_of(r: &Value) -> String {
    match &r["iIdProduit"] {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

fn qty(r: &Value) -> u32 {
    r["iQuantitePanier"].as_u64().unwrap_or(0) as u32
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn label(r: &Value) -> String {
    decode(&format!("{} {}", text(&r["sLibelleLigne1"]).trim(), text(&r["sLibelleLigne2"]).trim()))
        .trim()
        .to_string()
}

/// Labels carry HTML entities: `Lait demi-&#233;cr&#233;m&#233;`.
fn decode(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let ch = end.and_then(|e| match &rest[1..e] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n => match n.strip_prefix("#x").or_else(|| n.strip_prefix("#X")) {
                Some(h) => u32::from_str_radix(h, 16).ok(),
                None => n.strip_prefix('#').and_then(|d| d.parse().ok()),
            }
            .and_then(char::from_u32),
        });
        match (ch, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

// ----------------------------------------------------------------- commands

pub fn search(query: &str, limit: u32, format: &str, brand: Option<&str>, available_only: bool) -> Result<()> {
    let store = store()?;
    let html = Leclerc::open(true)?.page(&store, query)?;
    let brand = brand.map(str::to_lowercase);
    let products: Vec<Value> = records(&html)
        .into_iter()
        .filter(|r| !available_only || r["iQteDisponible"].as_u64().unwrap_or(0) > 0)
        // No brand field: it is part of the label.
        .filter(|r| brand.as_ref().is_none_or(|b| label(r).to_lowercase().contains(b)))
        .take(limit as usize)
        .collect();

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&json!({"products": products, "total": products.len()}))?);
        return Ok(());
    }
    if products.is_empty() {
        println!("No products found for '{}'.", query);
        return Ok(());
    }
    println!("Found {} products for '{}' at {}:\n", products.len(), query, store.name);
    for r in &products {
        println!("{}", label(r).bold());
        println!("  Price: {}", text(&r["sPrixUnitaire"]));
        let promo = text(&r["sPrixPromo"]);
        if !promo.is_empty() && !promo.starts_with("0,00") {
            println!("  Promo: {}", promo.green());
        }
        if !text(&r["sPrixParUniteDeMesure"]).is_empty() {
            println!("  Unit price: {}", text(&r["sPrixParUniteDeMesure"]));
        }
        let status = if r["iQteDisponible"].as_u64().unwrap_or(0) > 0 { "In stock".green() } else { "Out of stock".red() };
        println!("  Status: {}", status);
        if qty(r) > 0 {
            println!("  In cart: {}", qty(r));
        }
        println!("  ID: {}\n", id_of(r).dimmed());
    }
    Ok(())
}

pub fn show_cart(format: &str) -> Result<()> {
    let store = store()?;
    let (lines, summary) = Leclerc::open(true)?.cart(&store)?;
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&json!({"items": lines, "cart": summary}))?);
        return Ok(());
    }
    if lines.is_empty() {
        println!("Your cart is empty.");
        return Ok(());
    }
    println!("{} ({})\n", "Shopping Cart".bold().underline(), store.name);
    for r in &lines {
        println!("  {} x{} - {}", label(r).bold(), qty(r), text(&r["sTotalAPayer"]));
        println!("      ID: {}", id_of(r).dimmed());
    }
    println!("\n{}", "─".repeat(40));
    println!("Products: {} items", summary["iQuantitePanier"]);
    println!("{}: {}", "Total".bold(), text(&summary["sTotalAPayer"]));
    if let Some(next) = summary["sDateProchainRetraitDisponible"].as_str() {
        println!("\nNext pickup: {}", next.green());
    }
    Ok(())
}

pub fn set_quantity(id: &str, quantity: u32) -> Result<()> {
    let store = store()?;
    let (held, cart) = Leclerc::open(true)?.set(&store, id, quantity)?;
    if held != quantity {
        bail!("Product {id} is at quantity {held}, not {quantity}: unknown ID or not enough stock.");
    }
    println!("{}", format!("Product {id}: quantity {quantity}.").green());
    println!("Cart: {} items, {}", cart["iQuantitePanier"], text(&cart["sTotalAPayer"]));
    Ok(())
}

pub fn clear_cart() -> Result<()> {
    let store = store()?;
    let mut lc = Leclerc::open(true)?;
    let (lines, _) = lc.cart(&store)?;
    if lines.is_empty() {
        println!("Cart is already empty.");
        return Ok(());
    }
    for (i, r) in lines.iter().enumerate() {
        if i > 0 {
            // DataDome flags bursts of cart writes.
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        if let Err(e) = lc.set(&store, &id_of(r), 0) {
            eprintln!("Warning: failed to remove {}: {}", id_of(r), e);
        }
    }
    println!("{}", format!("Removed {} items from cart.", lines.len()).green());
    Ok(())
}

/// List drives near `query`, or remember the one whose id is `pick`.
pub fn store_cmd(query: Option<&str>, pick: Option<&str>) -> Result<()> {
    let Some(query) = query else {
        let s = store()?;
        println!("{} ({}, {})", s.name.bold(), s.id, s.host.dimmed());
        return Ok(());
    };
    let points = Leclerc::open(true)?.find_stores(query)?;
    let host = |p: &Value| {
        let u = p["urlSiteCourse"].as_str().filter(|u| !u.is_empty()).or(p["urlBase"].as_str());
        u.and_then(|u| url::Url::parse(u).ok()).and_then(|u| u.host_str().map(String::from))
    };
    let str_of = |v: &Value| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };

    if let Some(pick) = pick {
        let p = points
            .iter()
            .find(|p| str_of(&p["noPL"]).as_deref() == Some(pick))
            .with_context(|| format!("No store {pick} near '{query}'."))?;
        let s = Store {
            id: pick.into(),
            no_pr: str_of(&p["noPR"]).unwrap_or_else(|| pick.into()),
            host: host(p).context("This store has no shopping site")?,
            name: text(&p["name"]).into(),
        };
        let mut config = Config::load()?;
        println!("{}", format!("Shopping at {} ({}).", s.name, s.host).green());
        config.leclerc_store = Some(s);
        return config.save();
    }

    if points.is_empty() {
        println!("No drive found near '{}'.", query);
        return Ok(());
    }
    for p in &points {
        let dist = p["distance"].as_f64().map(|d| format!("{d:.1} km")).unwrap_or_default();
        let kind = text(&p["serviceType"]).to_lowercase();
        println!("{:<8} {:<40} {:<10} {}", str_of(&p["noPL"]).unwrap_or_default(), text(&p["name"]).bold(), kind, dist.dimmed());
    }
    println!("\n{}", format!("Pick one: carrefour --provider leclerc store {query} --pick <id>").dimmed());
    Ok(())
}

/// A visible window on the store, where the user signs in (and solves a
/// DataDome captcha if one shows up). The cookies are saved when it closes.
pub fn login() -> Result<()> {
    let store = store()?;
    let mut lc = Leclerc::open(false)?;
    lc.browser.open(&format!("{}/", store.base()))?;
    println!("Sign in to Leclerc Drive in the Chrome window, then press Enter here.");
    std::io::stdin().read_line(&mut String::new())?;
    let html = lc.page(&store, NO_MATCH)?;
    if html.contains("\"fClientConnecte\":true") {
        println!("{}", "Signed in!".green());
    } else {
        println!("{}", "Not signed in: the cart stays anonymous to this CLI.".yellow());
    }
    Ok(())
}

pub fn status() -> Result<()> {
    let store = store()?;
    println!("Store: {} ({})", store.name, store.id);
    let html = Leclerc::open(true)?.page(&store, NO_MATCH)?;
    if html.contains("\"fClientConnecte\":true") {
        println!("{}", "Authenticated".green().bold());
    } else {
        println!("{}", "Anonymous session (not signed in).".yellow());
    }
    Ok(())
}

pub fn logout() -> Result<()> {
    let _ = std::fs::remove_file(jar_path()?);
    let _ = std::fs::remove_dir_all(config_dir()?.join("leclerc-profile"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pages() {
        let html = r#"<script>init('x', {"objContenu":{"lstElements":[{"objElement":{"iIdProduit":2612,"sType":"Produit","sLibelleLigne1":"Lait demi-&#233;cr&#233;m&#233; ","sLibelleLigne2":"Brique - 6x1L","iQuantitePanier":2}},{"objElement":{"iIdProduit":2612,"sType":"Produit"}}]}, function(){ return {}; });
            var p = {"sType":"Panier","lstProduitsLight":[{"iIdProduit":2612,"iQtePanier":2}],"sTotalAPayer":"11,88 €"};</script>"#;
        let r = records(html);
        assert_eq!(r.len(), 1);
        assert_eq!(label(&r[0]), "Lait demi-écrémé Brique - 6x1L");
        assert_eq!((id_of(&r[0]), qty(&r[0])), ("2612".into(), 2));
        let cart = enclosing(html, html.find("\"lstProduitsLight\"").unwrap()).unwrap();
        assert_eq!(cart["sTotalAPayer"], "11,88 €");
        assert_eq!(decode("a &amp; b &#x41; &bogus c &"), "a & b A &bogus c &");
    }
}
