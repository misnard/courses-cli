//! WIP, NOT VERIFIED LIVE: DataDome answers headless Chrome with its
//! interstitial (HTTP 403, `t=it`) that never clears, even right after a
//! visible window passed it. Only the store finder's 403 was ever observed;
//! the mapping below follows the reference projects. `ITM_DUMP=<dir>` writes
//! raw answers there, for the next discovery session.
//!
//! Intermarché Drive. Endpoints from
//! https://github.com/nicolasestrem/mcp-intermarche-drive (MIT), see its
//! docs/api-notes.md, and the docs of
//! https://github.com/NicolasGuilloux/intermarche-mcp (no license file; only
//! its docs/api-reference.md was read, for the store finder).
//!
//! Every call goes through the site's Next.js proxy `/api/service/<service>/...`
//! (`x-service-name` names the service). DataDome guards it.
//!
//! - stores: GET  pdvs/v1/stores?scope=AUTOCOMPLETE&postalCodeOrCity=
//! - search: POST produits/v4/pdvs/{store}/products/byKeywordAndCategory
//! - cart:   POST panier/v1/stores/{store}/carts?anonymousCartId=..., a sync:
//!   no event reads the cart, a `QUANTITY` event (absolute target) writes it;
//!   both answer the whole cart.

use super::{save_store, store, str_of, Cart, CartLine, Product, Provider, Shop, Store, Web};
use anyhow::Result;
use serde_json::{json, Value};

const API: &str = "https://www.intermarche.com/api/service";
const HOME: &str = "https://www.intermarche.com/";
const PARK: &str = "https://www.intermarche.com/robots.txt";

pub struct Intermarche {
    web: Web,
}

/// Discovery only: write a site answer to the scratch dir.
fn dump(name: &str, v: &Value) {
    if let Ok(d) = std::env::var("ITM_DUMP") {
        let _ = std::fs::write(format!("{d}/{name}.json"), serde_json::to_string_pretty(v).unwrap());
    }
}

impl Intermarche {
    pub fn open(headless: bool) -> Result<Intermarche> {
        Ok(Intermarche { web: Web::open(Provider::Intermarche, "intermarche.com", headless)? })
    }

    fn call(&mut self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let service = path.split('/').nth(1).unwrap_or("");
        let h = [("x-red-device", "red_fo_desktop"), ("x-red-version", "3"), ("x-service-name", service)];
        let v = self.web.json(PARK, method, &format!("{API}{path}"), &h, body)?;
        dump(service, &v);
        Ok(v)
    }

    /// One cart sync. The anonymous cart id is ours to choose (the site
    /// makes a random UUID too); it lives with the picked store.
    fn sync(&mut self, events: Value, last: Option<&Value>) -> Result<Value> {
        let mut s = store(Provider::Intermarche)?;
        let id = match s.data["cart"].as_str() {
            Some(id) => id.to_string(),
            None => {
                let id = uuid();
                s.data["cart"] = json!(id);
                save_store(Provider::Intermarche, s.clone())?;
                id
            }
        };
        let path = format!("/panier/v1/stores/{}/carts?actions=VALUATION,ANIMATIONS&isActiveAnonymousPersistence=true&anonymousCartId={id}", s.id);
        let now = iso_now();
        let last = last.map_or(json!({"carts": [], "synchronizeDateTime": now}), |c| {
            json!({"carts": c["carts"], "synchronizeDateTime": c["synchronizeDateTime"]})
        });
        self.call("POST", &path, Some(json!({"customerDateTime": now, "events": events, "lastSynchronizedCart": last})))
    }
}

impl Shop for Intermarche {
    fn login(&mut self) -> Result<()> {
        self.web.login(HOME)
    }

    fn find_stores(&mut self, query: &str) -> Result<Vec<Store>> {
        let q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let v = self.call("GET", &format!("/pdvs/v1/stores?scope=AUTOCOMPLETE&postalCodeOrCity={q}"), None)?;
        Ok(stores(&v))
    }

    fn search(&mut self, query: &str, limit: u32) -> Result<Vec<Product>> {
        let s = store(Provider::Intermarche)?;
        let body = json!({"keyword": query, "page": 1, "size": limit.clamp(1, 50), "filtres": [], "tri": "pertinence", "ordreTri": null, "catalog": ["PDV"]});
        let v = self.call("POST", &format!("/produits/v4/pdvs/{}/products/byKeywordAndCategory", s.id), Some(body))?;
        Ok(v["produits"].as_array().into_iter().flatten().map(product).collect())
    }

    fn cart(&mut self) -> Result<Cart> {
        Ok(cart(&self.sync(json!([]), None)?))
    }

    fn set_quantity(&mut self, id: &str, quantity: u32) -> Result<u32> {
        let current = self.sync(json!([]), None)?;
        let event = json!({"type": "QUANTITY", "catalog": "PDV", "itemId": id, "quantity": quantity, "dateTime": iso_now()});
        let after = self.sync(json!([event]), Some(&current))?;
        Ok(cart(&after).lines.iter().find(|l| l.product.id == id).map_or(0, |l| l.quantity))
    }
}

// ------------------------------------------------------------------ parsing

fn stores(v: &Value) -> Vec<Store> {
    v["storeInfoList"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let a = &p["address"];
            let town = a["townLabel"].as_str().unwrap_or("");
            Some(Store {
                // The cart API wants the 5-digit form.
                id: format!("{:0>5}", str_of(&p["storeId"])?),
                name: p["name"].as_str().map_or_else(|| format!("Intermarché {town}"), String::from),
                info: format!("{}, {} {} {town}", p["modelLabel"].as_str().unwrap_or(""), a["address"].as_str().unwrap_or(""), a["postCode"].as_str().unwrap_or("")),
                data: Value::Null,
            })
        })
        .collect()
}

fn product(r: &Value) -> Product {
    let unit = r["unitePrixVente"]["value"].as_str().unwrap_or("");
    Product {
        // `itemId` is what the cart takes; the EAN is not interchangeable.
        id: str_of(&r["itemId"]).or_else(|| str_of(&r["identifier"])).unwrap_or_default(),
        name: r["libelle"].as_str().unwrap_or("").into(),
        brand: r["marque"].as_str().map(String::from),
        price: r["prix"].as_f64(),
        unit_price: r["prixKg"].as_f64().map(|p| format!("{} {unit}", format!("{p:.2}").replace('.', ","))),
        packaging: r["conditionnement"].as_str().map(String::from),
        ean: str_of(&r["produitEan13"]),
        available: r["dispoCataloguePdv"].as_bool().unwrap_or_else(|| r["stock"].as_f64().unwrap_or(0.0) > 0.0),
        ..Default::default()
    }
}

fn cart(v: &Value) -> Cart {
    let lines = v["carts"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|c| c["items"].as_array().into_iter().flatten())
        .map(|l| {
            let mut p = product(&l["item"]);
            p.id = str_of(&l["id"]).unwrap_or(p.id);
            let quantity = l["quantity"].as_u64().unwrap_or(0) as u32;
            p.in_cart = quantity;
            CartLine { product: p, quantity, total: l["amount"].as_f64() }
        })
        .collect();
    Cart { lines, total: v["amount"].as_f64(), note: None }
}

// ------------------------------------------------------------------- helpers

// ponytail: std's per-process random SipHash keys, not a CSPRNG; fine for a cart id.
fn uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    let r = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    let h = format!("{:016x}{:016x}", r(), r());
    format!("{}-{}-4{}-a{}-{}", &h[..8], &h[8..12], &h[13..16], &h[17..20], &h[20..])
}

/// UTC now as `2026-10-18T09:05:00Z` (days to civil date: H. Hinnant's algorithm).
fn iso_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    iso(secs)
}

fn iso(secs: u64) -> String {
    let (days, t) = ((secs / 86400) as i64, secs % 86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_772_193_600), "2026-02-27T12:00:00Z");
        assert_eq!(iso(951_782_400), "2000-02-29T00:00:00Z");
        let u = uuid();
        assert_eq!((u.len(), &u[14..15], &u[19..20]), (36, "4", "a"));
        assert_ne!(u, uuid());
    }
}
