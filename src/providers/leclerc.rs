//! E.Leclerc Drive. Endpoints from
//! https://github.com/skunkobi/mcp-leclerc-drive (MIT), see its docs/api-capture.md.
//!
//! DataDome guards every leclercdrive.fr host; the headless Chrome of
//! `browser.rs` gets through it as it does Cloudflare, as long as requests
//! leave from a real HTML page (`Web::send`'s `page`). Store pages are ASP.NET
//! HTML with the product records and the cart embedded as JSON objects; every
//! cart write is one form POST to `panier.aspx?op=1`.

use super::{decode_html, parse_euros, store, str_of, Cart, CartLine, Product, Provider, Shop, Store, Web};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

const LOCATOR: &str = "https://api-recherchemagasins.leclercdrive.fr/API_RechercheMagasins/api/v1";
/// Search that matches nothing, so the page only carries the cart's records.
const NO_MATCH: &str = "zzzznomatchzzz";

pub struct Leclerc {
    web: Web,
}

/// The store's API path; its shopping host varies: fd8-, fd9-, fd14-courses.leclercdrive.fr...
fn base(s: &Store) -> String {
    let host = s.data["host"].as_str().unwrap_or("");
    format!("https://{host}/magasin-{}-{}", s.id, s.data["no_pr"].as_str().unwrap_or(&s.id))
}

impl Leclerc {
    pub fn open(headless: bool) -> Result<Leclerc> {
        Ok(Leclerc { web: Web::open(Provider::Leclerc, "leclercdrive.fr", headless)? })
    }

    fn send(&mut self, s: &Store, method: &str, url: &str, form: Option<String>) -> Result<String> {
        let headers: &[(&str, &str)] = match form {
            Some(_) => &[("content-type", "application/x-www-form-urlencoded; charset=UTF-8"), ("x-requested-with", "XMLHttpRequest")],
            None => &[],
        };
        self.web.send(&format!("{}/", base(s)), method, url, headers, form)
    }

    /// A search page of the store; it also embeds the cart.
    fn page(&mut self, s: &Store, query: &str) -> Result<String> {
        let url = url::Url::parse_with_params(&format!("{}/recherche.aspx", base(s)), [("TexteRecherche", query)])?;
        let html = self.send(s, "GET", url.as_str(), None)?;
        // Every real store page carries the cart summary; DataDome's soft
        // challenge and the "session expirée" page (another drive) do not.
        if !html.contains("\"lstProduitsLight\"") {
            let login = Provider::Leclerc.cmd();
            if html.contains("session a expir") || html.contains("sessionexpiree") {
                bail!("Leclerc session expired or bound to another drive; run '{login} auth login'.");
            }
            bail!("Unexpected Leclerc page (DataDome challenge?); run '{login} auth login'.");
        }
        Ok(html)
    }

    fn locate(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        let url = url::Url::parse_with_params(&format!("{LOCATOR}{path}"), params)?;
        self.web.json("https://www.leclercdrive.fr/", "GET", url.as_str(), &[], None)
    }
}

impl Shop for Leclerc {
    fn search(&mut self, query: &str, _limit: u32) -> Result<Vec<Product>> {
        let html = self.page(&store(Provider::Leclerc)?, query)?;
        Ok(records(&html).iter().map(product).collect())
    }

    fn cart(&mut self) -> Result<Cart> {
        let html = self.page(&store(Provider::Leclerc)?, NO_MATCH)?;
        let lines = records(&html)
            .iter()
            .filter(|r| qty(r) > 0)
            .map(|r| CartLine { product: product(r), quantity: qty(r), total: r["rTotalAPayer"].as_f64() })
            .collect();
        let summary = html.find("\"lstProduitsLight\"").and_then(|i| enclosing(&html, i)).unwrap_or_default();
        Ok(Cart {
            lines,
            total: summary["sTotalAPayer"].as_str().and_then(parse_euros),
            note: summary["sDateProchainRetraitDisponible"].as_str().map(|d| format!("Next pickup: {d}")),
        })
    }

    /// `iQuantite` is the target, but the action must say which way it goes:
    /// 1 adds, 2 removes.
    fn set_quantity(&mut self, id: &str, quantity: u32) -> Result<u32> {
        let s = store(Provider::Leclerc)?;
        let current = records(&self.page(&s, NO_MATCH)?).iter().find(|r| id_of(r) == id).map(qty).unwrap_or(0);
        let action = if quantity >= current { 1 } else { 2 };
        let d = json!({"eTypeAction": action, "iIdProduit": id, "iQuantite": quantity, "sNoPointLivraison": s.id});
        let form = url::form_urlencoded::Serializer::new(String::new()).append_pair("d", &d.to_string()).finish();
        let text = self.send(&s, "POST", &format!("{}/panier.aspx?op=1", base(&s)), Some(form))?;
        let events: Vec<Value> = serde_json::from_str(&text)
            .with_context(|| format!("Unexpected cart response: {}", text.chars().take(120).collect::<String>()))?;
        // An unknown id still gets a `Produit{id}` event, at quantity 0.
        let key = format!("Produit{id}");
        Ok(events.iter().find(|e| e["sIdUnique"] == key.as_str()).map(|e| qty(&e["objElement"])).unwrap_or(0))
    }

    fn find_stores(&mut self, query: &str) -> Result<Vec<Store>> {
        let auto = self.locate("/autocomplete", &[("search", query), ("provider", "Woosmap")])?;
        let Some(place) = auto["postalCodes"].get(0) else { return Ok(vec![]) };
        let c = self.locate("/autocomplete/coordinates", &[("id", place["id"].as_str().unwrap_or("")), ("provider", "Woosmap")])?;
        let coord = |k: &str| c[k].as_f64().or(c["coordinates"][k].as_f64()).unwrap_or(0.0).to_string();
        let (lat, lng) = (coord("latitude"), coord("longitude"));
        let cp = place["postalCode"].as_str().unwrap_or(query);
        let near = self.locate("/MapPoint/nearby", &[("latitude", &lat), ("longitude", &lng), ("postalCode", cp)])?;
        let host = |p: &Value| {
            let u = p["urlSiteCourse"].as_str().filter(|u| !u.is_empty()).or(p["urlBase"].as_str());
            u.and_then(|u| url::Url::parse(u).ok()).and_then(|u| u.host_str().map(String::from))
        };
        Ok(near["points"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let id = str_of(&p["noPL"])?;
                let dist = p["distance"].as_f64().map(|d| format!(", {d:.1} km")).unwrap_or_default();
                Some(Store {
                    info: format!("{}{dist}", p["serviceType"].as_str().unwrap_or("").to_lowercase()),
                    name: p["name"].as_str().unwrap_or("").into(),
                    // noPR equals noPL for a drive, differs for a pedestrian relay.
                    data: json!({"host": host(p)?, "no_pr": str_of(&p["noPR"]).unwrap_or_else(|| id.clone())}),
                    id,
                })
            })
            .collect())
    }

    fn login(&mut self) -> Result<()> {
        let url = format!("{}/", base(&store(Provider::Leclerc)?));
        self.web.login(&url)
    }

    fn signed_in(&mut self) -> Result<bool> {
        Ok(self.page(&store(Provider::Leclerc)?, NO_MATCH)?.contains("\"fClientConnecte\":true"))
    }
}

// ------------------------------------------------------------------ parsing

fn product(r: &Value) -> Product {
    Product {
        id: id_of(r),
        name: label(r),
        price: r["nrPVUnitaireTTC"].as_f64(),
        unit_price: r["sPrixParUniteDeMesure"].as_str().filter(|s| !s.is_empty()).map(String::from),
        available: r["iQteDisponible"].as_u64().unwrap_or(0) > 0,
        in_cart: qty(r),
        ..Default::default()
    }
}

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
    str_of(&r["iIdProduit"]).unwrap_or_default()
}

fn qty(r: &Value) -> u32 {
    r["iQuantitePanier"].as_u64().unwrap_or(0) as u32
}

fn label(r: &Value) -> String {
    let line = |k: &str| r[k].as_str().unwrap_or("").trim().to_string();
    // Labels carry HTML entities: `Lait demi-&#233;cr&#233;m&#233;`.
    decode_html(&format!("{} {}", line("sLibelleLigne1"), line("sLibelleLigne2"))).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pages() {
        let html = r#"<script>init('x', {"objContenu":{"lstElements":[{"objElement":{"iIdProduit":2612,"sType":"Produit","sLibelleLigne1":"Lait demi-&#233;cr&#233;m&#233; ","sLibelleLigne2":"Brique - 6x1L","iQuantitePanier":2,"nrPVUnitaireTTC":5.94,"iQteDisponible":3}},{"objElement":{"iIdProduit":2612,"sType":"Produit"}}]}, function(){ return {}; });
            var p = {"sType":"Panier","lstProduitsLight":[{"iIdProduit":2612,"iQtePanier":2}],"sTotalAPayer":"11,88 €"};</script>"#;
        let r = records(html);
        assert_eq!(r.len(), 1);
        let p = product(&r[0]);
        assert_eq!((p.id.as_str(), p.name.as_str()), ("2612", "Lait demi-écrémé Brique - 6x1L"));
        assert_eq!((p.price, p.available, p.in_cart), (Some(5.94), true, 2));
        let cart = enclosing(html, html.find("\"lstProduitsLight\"").unwrap()).unwrap();
        assert_eq!(cart["sTotalAPayer"], "11,88 €");
    }
}
