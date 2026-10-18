//! Auchan Drive. Endpoints from https://github.com/MrRaph/mcp-auchan-drive
//! (MIT), see its docs/api-capture.md, and https://github.com/ncleton/auchan-mcp (MIT).
//!
//! DataDome guards www.auchan.fr; requests leave from its home page. The site
//! is server-rendered: search results are HTML cards (`/recherche`), the store
//! locator answers HTML fragments (`Accept: application/crest`). The drive is
//! a server-side "journey" (`POST /journey/update`) that the site's JS names
//! in the `lark-journey` cookie; we set that cookie the same way. The cart is
//! JSON, named by the `lark-cart` cookie; every write is one `POST /cart/update`.
//!
//! Product ids are `<productId>:<offerId>`: a cart line needs the drive's
//! offer, and a line added with the productId alone gets no price.

use super::{decode_html, save_store, store, Cart, CartLine, Product, Provider, Shop, Store, Web};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

const BASE: &str = "https://www.auchan.fr";
const HOME: &str = "https://www.auchan.fr/";
const XHR: (&str, &str) = ("x-requested-with", "XMLHttpRequest");
/// Search that matches nothing: still a full page, with the header.
const NO_MATCH: &str = "zzzznomatchzzz";

pub struct Auchan {
    web: Web,
}

impl Auchan {
    pub fn open(headless: bool) -> Result<Auchan> {
        Ok(Auchan { web: Web::open(Provider::Auchan, "auchan.fr", headless)? })
    }

    fn cookie(&mut self, name: &str) -> Result<Option<String>> {
        let jar = self.web.browser.cookies("auchan.fr")?;
        Ok(jar.iter().find(|c| c["name"] == name).and_then(|c| c["value"].as_str().map(String::from)))
    }

    /// A cookie the site's own JS would set.
    fn set_cookie(&mut self, name: &str, value: &str) -> Result<()> {
        self.web.browser.set_cookies(json!([{"name": name, "value": value, "domain": "www.auchan.fr", "path": "/"}]))
    }

    /// The picked drive, made the session's. Its journey is created once and
    /// kept in the config, so a new Chrome profile gets it back.
    fn drive(&mut self) -> Result<Store> {
        let mut s = store(Provider::Auchan)?;
        if s.data["journey"].as_str().is_none() {
            let seller = s.data["seller"].as_str().unwrap_or("").to_string();
            let form = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("offeringContext.seller.id", &seller)
                .append_pair("offeringContext.channels[0]", "PICK_UP")
                .append_pair("offeringContext.storeReference", &s.id)
                .finish();
            let headers = [("content-type", "application/x-www-form-urlencoded"), ("accept", "application/json"), XHR];
            let text = self.web.send(HOME, "POST", &format!("{BASE}/journey/update"), &headers, Some(form))?;
            let j: Value = serde_json::from_str(&text).context("Unexpected answer from /journey/update")?;
            let ok = j["activeContexts"].as_array().into_iter().flatten().any(|c| c["context"]["seller"]["id"] == seller.as_str());
            let Some(id) = j["id"].as_str().filter(|_| ok) else { bail!("Auchan did not accept drive {}.", s.id) };
            s.data["journey"] = json!(id);
            save_store(Provider::Auchan, s.clone())?;
        }
        let journey = s.data["journey"].as_str().unwrap_or("").to_string();
        self.set_cookie("lark-journey", &journey)?;
        Ok(s)
    }

    fn page(&mut self, query: &str) -> Result<String> {
        let url = url::Url::parse_with_params(&format!("{BASE}/recherche"), [("text", query)])?;
        let html = self.web.send(HOME, "GET", url.as_str(), &[], None)?;
        if !html.contains("class=\"context-header\"") {
            bail!("Unexpected Auchan page (DataDome challenge?); run '{} auth login'.", Provider::Auchan.cmd());
        }
        Ok(html)
    }

    fn cart_json(&mut self) -> Result<Value> {
        // Without its id, /cart answers a new empty cart.
        let id = self.cookie("lark-cart")?;
        let url = match &id {
            Some(id) => format!("{BASE}/cart?cartId={id}"),
            None => format!("{BASE}/cart"),
        };
        let cart = self.web.json(HOME, "GET", &url, &[XHR], None)?;
        if id.is_none() {
            // ponytail: a fresh profile before the site's JS named a cart; keep the new one.
            if let Some(new) = cart["cart"]["id"].as_str() {
                self.set_cookie("lark-cart", new)?;
            }
        }
        Ok(cart)
    }
}

impl Shop for Auchan {
    fn search(&mut self, query: &str, _limit: u32) -> Result<Vec<Product>> {
        self.drive()?;
        let html = self.page(query)?;
        if !html.contains("context-header__pos\"") {
            bail!("Auchan forgot the picked drive; pick it again: '{} store <postal code> --pick <id>'.", Provider::Auchan.cmd());
        }
        Ok(products(&html))
    }

    fn cart(&mut self) -> Result<Cart> {
        self.drive()?;
        Ok(cart_of(&self.cart_json()?))
    }

    /// One write for add, update and remove: the line's `id` when it is
    /// already in the cart (else the site adds a second line), 0 removes.
    fn set_quantity(&mut self, id: &str, quantity: u32) -> Result<u32> {
        let Some((product, offer)) = id.split_once(':').filter(|(p, o)| !p.is_empty() && !o.is_empty()) else {
            bail!("'{id}' is not an Auchan product id: use the <product>:<offer> id that search prints.");
        };
        let s = self.drive()?;
        let before = self.cart_json()?;
        let cart = &before["cart"];
        let line = cart["cart"]["items"].as_array().into_iter().flatten().find(|l| l["productId"] == product);
        if line.is_none() && quantity == 0 {
            return Ok(0);
        }
        let mut item = json!({"productId": product, "offerId": offer, "sellerId": s.data["seller"], "sellerType": "GROCERY",
            "desiredQuantity": quantity, "desiredType": "DEFAULT"});
        if let Some(l) = line {
            item["id"] = l["id"].clone();
        }
        let consent = self.cookie("lark-consentId")?.unwrap_or_default();
        let body = json!({"cartId": cart["id"], "items": [item], "consentId": consent, "reservationId": null, "mbaAvailabilityNeeded": true});
        let after = self.web.json(HOME, "POST", &format!("{BASE}/cart/update"), &[XHR], Some(body))?;
        let key = format!("{product}:");
        Ok(cart_of(&after).lines.iter().find(|l| l.product.id.starts_with(&key)).map_or(0, |l| l.quantity))
    }

    fn find_stores(&mut self, query: &str) -> Result<Vec<Store>> {
        let url = url::Url::parse_with_params(&format!("{BASE}/geocoding/autocomplete"), [("query", query)])?;
        let crest = |renderer: &'static str| [("accept", "application/crest"), ("x-crest-renderer", renderer), XHR];
        let html = self.web.send(HOME, "GET", url.as_str(), &crest("geocoding-renderer"), None)?;
        let Some(place) = html.split("<li ").nth(1) else { return Ok(vec![]) };
        let a = |k: &str| attr(place, k).unwrap_or_default();
        let params = [
            ("address.zipcode", a("data-zipcode")),
            ("address.city", a("data-city")),
            ("address.country", a("data-country")),
            ("location.latitude", a("data-latitude")),
            ("location.longitude", a("data-longitude")),
            ("accuracy", a("data-accuracy")),
            ("position", "1".into()),
            ("sellerType", "GROCERY".into()),
            ("filters.pos", "".into()),
            ("filters.slots", "".into()),
            ("filters.validStoreReferences", "".into()),
            ("channels", "PICK_UP,SHIPPING".into()),
        ];
        let url = url::Url::parse_with_params(&format!("{BASE}/offering-contexts"), &params)?;
        Ok(stores(&self.web.send(HOME, "GET", url.as_str(), &crest("journey-renderer"), None)?))
    }

    fn login(&mut self) -> Result<()> {
        self.web.login(HOME)
    }

    /// Every page's header script says it: `window.G.initialAuthInfo = {"authenticated":false,...}`.
    fn signed_in(&mut self) -> Result<bool> {
        Ok(self.page(NO_MATCH)?.contains("\"authenticated\":true"))
    }
}

// ------------------------------------------------------------------ parsing

/// Product cards of a search page.
fn products(html: &str) -> Vec<Product> {
    html.split("<article")
        .skip(1)
        .filter_map(|card| {
            let card = &card[..card.find("</article>").unwrap_or(card.len())];
            // Sponsored carousels and banners sit among the results.
            let class = attr(card, "class")?;
            if !class.starts_with("product-thumbnail") || !class.contains("list__item") {
                return None;
            }
            let product = attr(card, "data-id")?;
            // Set only when the drive sells it.
            let offer = attr(card, "data-current-offer-id");
            let enabled = after(card, "class=\"quantity-selector").and_then(|s| attr(s, "data-disable-button")).is_some_and(|d| d != "true");
            let meta = |prop: &str| after(card, &format!("itemprop=\"{prop}\"")).and_then(|r| attr(r, "content"));
            Some(Product {
                available: enabled && offer.is_some() && meta("availability").is_some_and(|a| a.ends_with("InStock")),
                id: offer.map_or(product.clone(), |o| format!("{product}:{o}")),
                name: text(card, "itemprop=\"name description\"", "</p>")?,
                brand: text(card, "itemprop=\"brand\"", "</strong>"),
                price: meta("price").and_then(|p| p.parse().ok()),
                unit_price: text(card, "<span data-seller-type=", "</span>").filter(|u| u.contains('/')),
                packaging: text(card, "class=\"product-attribute\"", "</span>"),
                ..Default::default()
            })
        })
        .collect()
}

/// Drives and pick-up points of an `/offering-contexts` fragment, nearest first.
fn stores(html: &str) -> Vec<Store> {
    let mut found: Vec<(f64, Store)> = html
        .split("journeyPosItem")
        .skip(1)
        .filter_map(|item| {
            // Its pick-up form; another one may offer home delivery from there.
            let form = item.split("<form").find(|f| f.contains("name=\"storeReference\""))?;
            // The others switch to a partner's site.
            if input(form, "availableInLark")? != "true" {
                return None;
            }
            let dist = text(item, "__pos-distance", "</div>").unwrap_or_default();
            let kind = attr(item, "data-type").unwrap_or_default().to_lowercase().replace('_', " ");
            let store = Store {
                id: input(form, "storeReference")?,
                name: text(item, "place-pos__name\"", "</span>")?,
                info: format!("{kind}, {dist}"),
                data: json!({"seller": input(form, "sellerId")?}),
            };
            Some((dist.trim_end_matches(" km").parse().unwrap_or(f64::MAX), store))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, s)| s).collect()
}

/// `/cart` and `/cart/update` answers. `cart.items` are the lines with an
/// offer at the drive, joined with their product and price.
fn cart_of(v: &Value) -> Cart {
    let c = &v["cart"];
    let lines: Vec<CartLine> = c["items"].as_array().into_iter().flatten().map(line).collect();
    // Not `sum()`: an empty f64 sum is -0.0.
    let sum = lines.iter().filter_map(|l| l.total).fold(0.0, |a, t| a + t);
    let site = c["prices"]["totalPrice"]["amount"].as_f64().map(|a| a / 100.0);
    // The site's total stays at 0 for lines it has not priced yet.
    Cart { total: Some(site.filter(|t| *t > 0.0).unwrap_or(sum)), lines, note: None }
}

fn line(it: &Value) -> CartLine {
    let quantity = it["offering"]["actualQuantity"].as_u64().or(it["desiredQuantity"].as_u64()).unwrap_or(0) as u32;
    let price = it["offer"]["price"]["amount"].as_f64().map(|a| a / 100.0);
    let p = &it["product"];
    let brand = p["brand"]["name"].as_str().map(String::from);
    let name = p["name"].as_str().unwrap_or("");
    CartLine {
        product: Product {
            id: format!("{}:{}", it["productId"].as_str().unwrap_or(""), it["offerId"].as_str().unwrap_or("")),
            name: brand.as_ref().map_or(name.to_string(), |b| format!("{b} {name}")),
            brand,
            price,
            ean: p["eans"][0].as_str().map(String::from),
            available: it["offer"]["availability"] == "AVAILABLE",
            in_cart: quantity,
            ..Default::default()
        },
        quantity,
        total: price.map(|p| p * quantity as f64),
    }
}

fn after<'a>(s: &'a str, marker: &str) -> Option<&'a str> {
    s.find(marker).map(|i| &s[i + marker.len()..])
}

/// First ` name="..."` attribute in `s`, decoded.
fn attr(s: &str, name: &str) -> Option<String> {
    let rest = after(s, &format!(" {name}=\""))?;
    Some(decode_html(&rest[..rest.find('"')?]))
}

/// Value of the form input `name`.
fn input(form: &str, name: &str) -> Option<String> {
    attr(after(form, &format!("name=\"{name}\""))?, "value")
}

/// Text of the element whose opening tag holds `marker`, up to `end`.
fn text(s: &str, marker: &str, end: &str) -> Option<String> {
    let rest = after(s, marker)?;
    let rest = &rest[rest.find('>')? + 1..];
    let mut plain = String::new();
    let mut in_tag = false;
    for c in rest[..rest.find(end)?].chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let t = decode_html(&plain).split_whitespace().collect::<Vec<_>>().join(" ");
    (!t.is_empty()).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pages() {
        let search = r#"<article class="product-thumbnail carousel__item" data-id="ad" data-current-offer-id="x"><p itemprop="name description">Ad</p></article>
            <article itemscope="itemscope" role="article" class="product-thumbnail list__item shadow--light " data-current-seller-type="GROCERY" data-current-offer-id="0026" data-id="fe94">
            <p class="product-thumbnail__description" itemprop="name description"><strong itemprop="brand">LACTEL</strong> Vita&apos;Vie Lait 12 vitamines </p>
            <div class="product-thumbnail__attributes"><span class="product-attribute" aria-label="Contenance : 6x1l"> 6x1l </span><span data-seller-type="GROCERY" data-offer-id="0026">1,19&#x20AC; / l</span></div>
            <meta itemprop="price" content="7.11"><meta itemprop="availability" content="https://schema.org/InStock">
            <div class="quantity-selector qa2c-wrapper
                " data-product-id="fe94" data-offer-id="0026" data-stock="188" data-disable-button="false" data-seller-id="b42f"></div></article>"#;
        let p = products(search);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].id.as_str(), p[0].name.as_str()), ("fe94:0026", "LACTEL Vita'Vie Lait 12 vitamines"));
        assert_eq!((p[0].brand.as_deref(), p[0].price, p[0].available), (Some("LACTEL"), Some(7.11), true));
        assert_eq!((p[0].unit_price.as_deref(), p[0].packaging.as_deref()), (Some("1,19€ / l"), Some("6x1l")));

        let locator = r#"<section><div class="journey-offering-context__wrapper journeyPosItem" data-id="5348" data-type="DRIVE" data-city="CALUIRE">
            <span class="place-pos__name">Auchan Drive Caluire</span><div class="journey-offering-context__pos-distance icon-direction">5.85 km</div>
            <form class="journeyChoice"><input name="availableInLark" value="true" type="hidden"><input name="sellerId" value="b42f" type="hidden"><input name="channels" value="PICK_UP" type="hidden"><input name="storeReference" value="527" type="hidden"></form></div>
            <div class="journey-offering-context__wrapper journeyPosItem" data-type="PICKUP_POINT"><span class="place-pos__name">Auchan Click&amp;collect Lyon</span><div class="journey-offering-context__pos-distance icon-direction">1.26 km</div>
            <form class="journeyChoice"><input name="availableInLark" value="false" type="hidden"><input name="sellerId" value="5bd7" type="hidden"><input name="storeReference" value="6085" type="hidden"></form></div>
            <div class="journey-offering-context__wrapper journeyPosItem" data-type="PICKUP_POINT"><span class="place-pos__name">Auchan Click&amp;collect Garibaldi</span><div class="journey-offering-context__pos-distance icon-direction">2.03 km</div>
            <form class="journeyChoice"><input name="availableInLark" value="true" type="hidden"><input name="sellerId" value="f075" type="hidden"><input name="storeReference" value="6079" type="hidden"></form>
            <form class="journeyChoice"><input name="availableInLark" value="true" type="hidden"><input name="sellerId" value="4053" type="hidden"><input name="channels" value="SHIPPING" type="hidden"></form></div></section>"#;
        let s = stores(locator);
        let got: Vec<_> = s.iter().map(|s| (s.id.as_str(), s.name.as_str(), s.info.as_str(), s.data["seller"].as_str().unwrap())).collect();
        assert_eq!(
            got,
            [
                ("6079", "Auchan Click&collect Garibaldi", "pickup point, 2.03 km", "f075"),
                ("527", "Auchan Drive Caluire", "drive, 5.85 km", "b42f")
            ]
        );

        let cart = json!({"cart": {"cart": {"id": "f332", "items": [{"id": "708e", "productId": "fe94", "desiredQuantity": 2}, {"id": "junk", "productId": "dead", "desiredQuantity": 1}]},
            "id": "f332", "prices": {"totalPrice": {"amount": 0, "currency": "EUR"}},
            "items": [{"id": "708e", "productId": "fe94", "offerId": "0026", "desiredQuantity": 2,
                "product": {"name": "Vita'Vie Lait 12 vitamines", "brand": {"name": "LACTEL"}, "eans": ["3428271240029"]},
                "offer": {"price": {"amount": 711, "currency": "EUR"}, "availability": "AVAILABLE"}}]}});
        let c = cart_of(&cart);
        assert_eq!(c.lines.len(), 1);
        let l = &c.lines[0];
        assert_eq!((l.product.id.as_str(), l.product.name.as_str(), l.quantity), ("fe94:0026", "LACTEL Vita'Vie Lait 12 vitamines", 2));
        assert_eq!((l.total, c.total, l.product.ean.as_deref()), (Some(14.22), Some(14.22), Some("3428271240029")));
    }
}
