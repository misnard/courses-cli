//! Super U (Courses U, www.coursesu.com). The search page and its product
//! tiles come from https://github.com/nmorvil/cli-drive-u (MIT); the store
//! finder from the site's own `prehome.js`.
//!
//! Salesforce Commerce Cloud (site `Sites-DigitalU-Site`) behind Cloudflare:
//! curl gets a 403, the in-Chrome fetch of `browser.rs` a 200. Everything is
//! HTML:
//! - `/recherche?q=`: one `<li data-tile>` per product, its record as JSON in
//!   `data-tc-product-tile`. Prices show once the session is bound to a store
//!   by `Stores-SetStoreWeb` (POST storeId + the page's `csrf_token`); the
//!   page's `tc_vars` then carry `"store_id":"<id>"`. Each page links the
//!   next 18 products in its `.infinite-scroll-placeholder`'s
//!   `data-grid-url` (`&start=18&sz=18&format=page-element`), which the
//!   site's infinite scroll (`categorypage.js`) GETs as an XHR.
//! - An out-of-stock tile (class `product-unavailable`) has no add-to-cart
//!   button (`data-bag`) and the price "unknown"; results list them last.
//! - `StoreLocator-Autocomplete?query=`: places (postal code, town,
//!   coordinates); `StoreLocator-GeolocateByCPandCity`: the stores near one.

use super::{decode_html, store, str_of, Product, Provider, Shop, Store, Web};
use anyhow::{bail, Result};
use serde_json::Value;

const HOME: &str = "https://www.coursesu.com/";
const SFCC: &str = "https://www.coursesu.com/on/demandware.store/Sites-DigitalU-Site/fr_FR";
const AJAX: &[(&str, &str)] = &[("x-requested-with", "XMLHttpRequest")];

pub struct Superu {
    web: Web,
}

impl Superu {
    pub fn open(headless: bool) -> Result<Superu> {
        Ok(Superu { web: Web::open(Provider::Superu, "coursesu.com", headless)? })
    }

    fn get(&mut self, url: &str, params: &[(&str, &str)], headers: &[(&str, &str)]) -> Result<String> {
        let url = url::Url::parse_with_params(url, params)?;
        self.web.send(HOME, "GET", url.as_str(), headers, None)
    }

    /// The search page for `query`, from a session bound to the picked store
    /// (prices are per store). Binding takes two more requests, once per session.
    fn search_page(&mut self, query: &str) -> Result<String> {
        let s = store(Provider::Superu)?;
        let bound = format!("\"store_id\":\"{}\"", s.id);
        let html = self.get(&format!("{HOME}recherche"), &[("q", query)], &[])?;
        if html.contains(&bound) {
            return Ok(html);
        }
        let Some(csrf) = between(&html, "name=\"csrf_token\" value=\"", "\"") else {
            bail!("Unexpected Super U page (bot check?); run '{} auth login'.", Provider::Superu.cmd());
        };
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("csrf_token", csrf)
            .append_pair("storeId", &s.id)
            .finish();
        let headers = [AJAX[0], ("content-type", "application/x-www-form-urlencoded; charset=UTF-8")];
        let r = self.web.send(HOME, "POST", &format!("{SFCC}/Stores-SetStoreWeb?format=ajax"), &headers, Some(form))?;
        if serde_json::from_str::<Value>(&r).ok().and_then(|v| v["error"].as_bool()) != Some(false) {
            bail!("Super U refused store {}: {}", s.id, r.chars().take(120).collect::<String>());
        }
        let html = self.get(&format!("{HOME}recherche"), &[("q", query)], &[])?;
        if !html.contains(&bound) {
            bail!("Super U did not switch to store {}.", s.id);
        }
        Ok(html)
    }
}

impl Shop for Superu {
    /// Loads further pages, as the site's infinite scroll does, until `limit`.
    fn search(&mut self, query: &str, limit: u32) -> Result<Vec<Product>> {
        let mut html = self.search_page(query)?;
        let mut found = products(&html);
        while found.len() < limit as usize {
            let Some(next) = next_page(&html) else { break };
            html = self.web.send(HOME, "GET", url::Url::parse(HOME)?.join(&next)?.as_str(), AJAX, None)?;
            let more = products(&html);
            if more.is_empty() {
                break;
            }
            found.extend(more);
            // A product can show again on the next page.
            let mut seen = std::collections::HashSet::new();
            found.retain(|p| seen.insert(p.id.clone()));
        }
        Ok(found)
    }

    fn find_stores(&mut self, query: &str) -> Result<Vec<Store>> {
        let places = self.get(&format!("{SFCC}/StoreLocator-Autocomplete?format=ajax"), &[("query", query)], AJAX)?;
        // The first place the site suggests, as its prehome does on Enter.
        let Some(place) = places.split("<li ").find(|li| li.contains("data-prehome-city-suggestion")) else {
            return Ok(vec![]);
        };
        let a = |k| attr(place, k).map(decode_html).unwrap_or_default();
        let params = [
            ("zip_code", a("data-city-zipcode")),
            ("city", a("data-city-name")),
            ("delivery_mode", "drive".into()),
            ("trustCoordinates", a("data-autocomplete-trust")),
            ("latitude", a("data-lat-input")),
            ("longitude", a("data-long-input")),
        ];
        let params: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let html = self.get(&format!("{SFCC}/StoreLocator-GeolocateByCPandCity?format=ajax"), &params, AJAX)?;
        Ok(stores(&html))
    }

    fn login(&mut self) -> Result<()> {
        self.web.login(&format!("{HOME}connexion?hasorigin=true"))
    }

    /// Store pages set `window.loggedIn`, `false` for a guest.
    fn signed_in(&mut self) -> Result<bool> {
        Ok(self.get(&format!("{HOME}recherche"), &[("q", "lait")], &[])?.contains("window.loggedIn = true;"))
    }
}

// ------------------------------------------------------------------ parsing

/// The text between `start` and the next `end`.
fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let s = &s[s.find(start)? + start.len()..];
    Some(&s[..s.find(end)?])
}

/// An attribute's raw (still entity-encoded) value in a tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    between(tag, &format!(" {name}=\""), "\"")
}

/// Visible text of an HTML fragment.
fn text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_html(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn products(html: &str) -> Vec<Product> {
    // A tile ends at its first `</li>` (it holds no list); the last one would
    // otherwise run to the end of the page.
    html.split("<li data-tile=").skip(1).filter_map(|t| product(t.split("</li>").next()?)).collect()
}

/// The URL of the next page of results, while the grid says there is one.
fn next_page(html: &str) -> Option<String> {
    let tag = between(html, "class=\"infinite-scroll-placeholder\"", ">")?;
    if !tag.contains("data-loading-state=\"unloaded\"") {
        return None;
    }
    between(tag, "data-grid-url=\"", "\"").map(decode_html)
}

fn product(tile: &str) -> Option<Product> {
    let r: Value = serde_json::from_str(&decode_html(attr(tile, "data-tc-product-tile")?)).ok()?;
    let s = |k: &str| r[k].as_str().filter(|v| !v.is_empty()).map(String::from);
    // "unknown" until the session has a store.
    let price = r["price"].as_str().and_then(|p| p.parse().ok());
    Some(Product {
        id: str_of(&r["id"])?,
        name: s("name")?.split_whitespace().collect::<Vec<_>>().join(" "),
        brand: s("brand"),
        price,
        unit_price: between(tile, "class=\"unit-info su-font-open\">", "<").map(text),
        nutriscore: between(tile, "alt=\"Nutriscore ", "\"").map(String::from),
        ean: s("EAN"),
        available: price.is_some() && tile.contains(" data-bag "),
        // ponytail: the tile's quantity box; unverified with a non-empty cart (the CLI cannot fill one).
        in_cart: between(tile, "name=\"Quantity\"", ">")
            .and_then(|t| attr(t, "value")?.parse::<f64>().ok())
            .map_or(0, |q| q as u32),
        ..Default::default()
    })
}

fn stores(html: &str) -> Vec<Store> {
    html.split("<li ")
        .filter_map(|li| {
            let id = attr(li, "data-store-id")?;
            let dist = between(li, "class=\"store-distance su-font-open\">", "</p>").map(text);
            let address = between(li, "class=\"store-address su-font-open\" data-cs-mask>", "<").map(text);
            Some(Store {
                id: id.into(),
                name: text(between(li, "data-store-name>", "<")?),
                info: [dist, address].into_iter().flatten().collect::<Vec<_>>().join(", "),
                ..Default::default()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pages() {
        let search = r#"<ul id="search-result-items">
<li data-tile="0" data-page-number="0.0" class='grid-tile small-6 medium-6 0' data-product-id="7634896" data-comment-wrapper data-tc-product-tile="{&quot;id&quot;:&quot;7634896&quot;,&quot;name&quot;:&quot;Lait UHT demi-&eacute;cr&eacute;m&eacute; - 6 bouteilles de 1L&quot;,&quot;EAN&quot;:&quot;3256225425617&quot;,&quot;brand&quot;:&quot;PRODUIT U&quot;,&quot;price&quot;:&quot;6.96&quot;,&quot;product_url_picture&quot;:&quot;https://static.coursesu.com/x.png?sw=388&amp;sh=388&quot;}">
<img width="57" height="30" src="https://www.coursesu.com/n.png" alt="Nutriscore B" />
<span aria-hidden="true" data-sup-product-price data-item-price="6.96" class="sale-price su-font-mulish" role="text">6,96 €</span>
<span class="unit-info-container">
    <span aria-hidden="true" class="unit-info su-font-open">1,16 &#8364;/l</span>
</span>
<div data-global-login-cta="productAddition" data-slot-tc data-bag class="product-button__bag icon-bag" data-pid="7634896">
<input type="number" name="Quantity" data-attr-quantity='1' data-max-quantity="6.0" value="2" inputmode="numeric" min="1" max="9999" autocomplete="off"/>
</div></li>
<li data-tile="25" data-page-number="25.0" class='grid-tile small-6 medium-6 17' data-product-id="5870443" data-comment-wrapper data-tc-product-tile="{&quot;id&quot;:&quot;5870443&quot;,&quot;name&quot;:&quot;Lait UHT biologique entier Bouteille 6x1L&quot;,&quot;EAN&quot;:&quot;3256222242651&quot;,&quot;brand&quot;:&quot;U BIO&quot;,&quot;price&quot;:&quot;unknown&quot;,&quot;quantity&quot;:1}">
<div class="product-tile product-unavailable product-tile-advantage-false master-product-false" data-itemid="5870443" data-product-tile data-product-order-unavailable="5870443">
<div class="product-unavailable-overlay">
<p class="product-unavailable-message su-font-open">Produit bient&ocirc;t disponible</p>
</div>
<div class="product-pricing  product-not-available " data-product-pricing>
<span class="su-btn su-btn-medium su-btn-primary-default similar-products-show-button" data-similar-products-show-button tabindex="0" role="button" data-relevant-sticky>
</span></div></div></li></ul>
<div class="infinite-scroll-placeholder"
data-loading-state="unloaded"
data-grid-url="/recherche?q=lait&amp;start=18&amp;sz=18&amp;format=page-element"
data-loading-state-prev="loaded"
data-grid-url-prev="/recherche?q=lait">
</div>
<div data-bag class="page-tail"></div>"#;
        let p = products(search);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].id.as_str(), p[0].name.as_str()), ("7634896", "Lait UHT demi-écrémé - 6 bouteilles de 1L"));
        assert_eq!((p[0].brand.as_deref(), p[0].ean.as_deref()), (Some("PRODUIT U"), Some("3256225425617")));
        assert_eq!((p[0].price, p[0].unit_price.as_deref(), p[0].nutriscore.as_deref()), (Some(6.96), Some("1,16 €/l"), Some("B")));
        // The second tile is out of stock: no add-to-cart button.
        assert!(p[0].available && !p[1].available);
        assert_eq!((p[0].in_cart, p[1].in_cart), (2, 0));
        assert_eq!(next_page(search).as_deref(), Some("/recherche?q=lait&start=18&sz=18&format=page-element"));
        let last = r#"<div class="infinite-scroll-placeholder" data-loading-state="loaded" data-grid-url="" data-loading-state-prev="" data-grid-url-prev="/recherche?q=lait&amp;start=432&amp;sz=18&amp;format=page-element" >"#;
        assert_eq!(next_page(last), None);

        let place = r#"<li id="350000" data-autocomplete-trust="true" tabindex="0" role="option" data-prehome-city-suggestion class="ab-prehome-search-suggestion su-font-open" data-link-index="0" data-city-zipcode="35000" data-city-name="Rennes" data-lat-input="48.111339" data-long-input="-1.68002" data-cs-mask>"#;
        assert_eq!((attr(place, "data-city-zipcode"), attr(place, "data-lat-input")), (Some("35000"), Some("48.111339")));

        let list = r#"<ul id="search-result-listbox" role="listbox" class="stores-results-list" data-stores-scroll data-slot-disabled="false">
            <li id="37910" class="store-container" role="option" data-city-zipcode="35000" data-city-name="Rennes" data-store-id="37910" data-store-position="1.0" data-wrapper-store-details>
                <div class="store-title-container">
                    <h3 class="store-name su-font-mulish su-font-weight-700" data-cs-mask data-store-name>U Express - Rennes Liberte</h3>
                    <p class="store-distance su-font-open"><span class="bold-text su-font-mulish">9,6</span> km</p>
                </div>
                    <div class="store-address su-font-open" data-cs-mask>27 rue poullain duparc, 35000 Rennes</div>
                <div class="store-delivery-option drive-option mode-RETRAIT  " data-delivery-zoneid="null" data-delivery-mode="RETRAIT" data-delivery-mode-store="37910"></div>
            </li></ul>"#;
        let s = stores(list);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].id.as_str(), s[0].name.as_str()), ("37910", "U Express - Rennes Liberte"));
        assert_eq!(s[0].info, "9,6 km, 27 rue poullain duparc, 35000 Rennes");
    }
}
