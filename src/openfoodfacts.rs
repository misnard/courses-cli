//! Nutri-Score from Open Food Facts (https://world.openfoodfacts.org, data
//! under the Open Database License), for products a site sells without one.
//! Anonymous, read-only calls:
//! - one batched search per 100 EANs on OFF's search service (search-a-licious):
//!   `GET https://search.openfoodfacts.org/search?q=code:(ean1 OR ean2 ...)&fields=code,nutriscore_grade`
//! - its index lags the database, so each EAN it does not know gets
//!   `GET https://world.openfoodfacts.org/api/v2/product/{ean}.json?fields=code,nutriscore_grade`
//!   (404 when OFF does not have it either).
//!
//! The world API's own batched `api/v2/search?code=...` answers 503 "not
//! available to anonymous users" most of the time, so it is not used.
//! Neither host sends CORS headers: each fetch leaves from that host's
//! `robots.txt`, in a Chrome of our own (`openfoodfacts-profile`).

use crate::browser::Browser;
use crate::config::config_dir;
use crate::providers::Product;
use crate::spec::Request;
use anyhow::{bail, Result};
use serde_json::{Map, Value};
use std::collections::HashMap;

const SEARCH: &str = "https://search.openfoodfacts.org";
const WORLD: &str = "https://world.openfoodfacts.org";

/// EAN → grade "A".."E", None when OFF has the product but no grade.
type Grades = HashMap<String, Option<String>>;

/// Fill `nutriscore` on products that have an EAN but no grade. Launches
/// Chrome only when there is something to look up; on error, still fills
/// what it found before.
pub fn fill_nutriscores(products: &mut [Product]) -> Result<()> {
    let mut codes: Vec<String> = products
        .iter()
        .filter(|p| lacks_grade(p))
        .filter_map(|p| p.ean.as_deref())
        // Digits only: the EAN goes into a query, and OFF codes are digits.
        .filter(|e| !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit()))
        .map(off_code)
        .collect();
    codes.sort();
    codes.dedup();
    if codes.is_empty() {
        return Ok(());
    }
    let mut found = Grades::new();
    let res = lookup(&codes, &mut found);
    fill(products, &found);
    res
}

fn lookup(codes: &[String], found: &mut Grades) -> Result<()> {
    let mut browser = Browser::launch(&config_dir()?.join("openfoodfacts-profile"), &[], true)?;
    for chunk in codes.chunks(100) {
        let q = format!("code:({})", chunk.join(" OR "));
        let url = url::Url::parse_with_params(&format!("{SEARCH}/search"), [("q", q.as_str()), ("fields", "code,nutriscore_grade"), ("page_size", "100")])?;
        found.extend(hits(&get(&mut browser, SEARCH, url.as_str())?));
    }
    // ponytail: one request per EAN the index misses, sequential; OFF allows
    // 100 product reads a minute, plenty for a search page.
    let missing: Vec<&String> = codes.iter().filter(|c| !found.contains_key(*c)).collect();
    for code in missing {
        let v = get(&mut browser, WORLD, &format!("{WORLD}/api/v2/product/{code}.json?fields=code,nutriscore_grade"))?;
        found.insert(code.clone(), grade(&v["product"]));
    }
    Ok(())
}

/// GET `url` from `host`'s robots.txt (tiny, same-origin). 404 is an answer:
/// the product API's "not found".
fn get(browser: &mut Browser, host: &str, url: &str) -> Result<Value> {
    // ponytail: OFF asks API clients for a descriptive User-Agent; an in-page
    // fetch cannot set one, so OFF sees plain Chrome. Every request of this
    // project runs in Chrome; a direct HTTP client would be the fix.
    let req = Request { method: "GET".into(), url: url.into(), headers: Map::new(), body: None };
    let res = browser.fetch_from(&format!("{host}/robots.txt"), &req)?;
    if !matches!(res.status, 200 | 404) {
        bail!("HTTP {} from {host}", res.status);
    }
    Ok(serde_json::from_str(&res.text)?)
}

/// No A..E grade: missing, or a site placeholder (Carrefour's "O" for brands
/// that do not display the Nutri-Score).
fn lacks_grade(p: &Product) -> bool {
    !matches!(p.nutriscore.as_deref(), Some("A" | "B" | "C" | "D" | "E"))
}

/// OFF's form of a barcode: no leading zeros, then padded to EAN-8 or EAN-13
/// (Carrefour sends EAN-8s as `0000059032823`, OFF knows `59032823`).
fn off_code(ean: &str) -> String {
    let c = ean.trim_start_matches('0');
    format!("{c:0>w$}", w = if c.len() <= 8 { 8 } else { 13 })
}

/// "a".."e" → "A".."E"; "unknown", "not-applicable" or none → None.
fn grade(product: &Value) -> Option<String> {
    let g = product["nutriscore_grade"].as_str()?;
    matches!(g, "a" | "b" | "c" | "d" | "e").then(|| g.to_uppercase())
}

fn hits(res: &Value) -> Grades {
    res["hits"].as_array().into_iter().flatten().filter_map(|h| Some((h["code"].as_str()?.to_string(), grade(h)))).collect()
}

fn fill(products: &mut [Product], found: &Grades) {
    for p in products.iter_mut().filter(|p| lacks_grade(p)) {
        if let Some(Some(g)) = p.ean.as_deref().and_then(|e| found.get(&off_code(e))) {
            p.nutriscore = Some(g.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_and_fills() {
        let search = json!({"hits": [
            {"code": "3017620422003", "nutriscore_grade": "e"},
            {"code": "59032823", "nutriscore_grade": "e"},
            {"code": "3245412345678", "nutriscore_grade": "unknown"},
            {"code": "0000000000000", "nutriscore_grade": "not-applicable"}
        ], "page": 1, "count": 4});
        let mut found = hits(&search);
        assert_eq!(found.len(), 4);
        assert_eq!(found["3245412345678"], None);
        // Product API: found, then 404.
        let product = json!({"code": "4255582819328", "product": {"code": "4255582819328", "nutriscore_grade": "d"}, "status": 1});
        found.insert("4255582819328".into(), grade(&product["product"]));
        let none = json!({"code": "3999999999999", "status": 0, "status_verbose": "product not found"});
        found.insert("3999999999999".into(), grade(&none["product"]));

        let p = |ean: Option<&str>, n: Option<&str>| Product { ean: ean.map(Into::into), nutriscore: n.map(Into::into), ..Default::default() };
        let mut products = [
            p(Some("3017620422003"), Some("O")),
            p(Some("0000059032823"), None),
            p(Some("4255582819328"), None),
            p(Some("3560070894222"), Some("A")),
            p(Some("3245412345678"), Some("O")),
            p(Some("3999999999999"), None),
            p(None, None),
        ];
        fill(&mut products, &found);
        let got: Vec<_> = products.iter().map(|p| p.nutriscore.as_deref()).collect();
        assert_eq!(got, [Some("E"), Some("E"), Some("D"), Some("A"), Some("O"), None, None]);
        assert_eq!(off_code("03017620422003"), "3017620422003");
        assert_eq!(off_code("012345678905"), "0012345678905");
    }
}
