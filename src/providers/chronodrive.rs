//! Chronodrive. The JSON API www.chronodrive.com itself calls, also used by
//! https://github.com/ncleton/chronodrive-mcp (MIT) and
//! https://github.com/arvernesmotion/Chronodrivemcp (no license file; read for
//! the endpoints only). Store finder and cart persistence come from the site's
//! own Nuxt bundle.
//!
//! Every call goes to `https://api.chronodrive.com/v1` from a tab parked on
//! that same origin: its CORS answer is `*` without credentials, which the
//! credentialed in-page fetch of `browser.rs` cannot use from another origin.
//! Each service has its own public `x-api-key`, read from the homepage's
//! `window.__NUXT__.config.public` (a second parked tab);
//! `x-chronodrive-site-id` names the store, `x-chronodrive-site-mode: DRIVE`.
//! - search: `GET /products?searchTerm=&page=&size=`
//! - stores: `GET /cities?searchTerm=<cp or city>` gives coordinates, then
//!   `GET /sites` lists every store (55 in Oct. 2026), sorted here by distance.
//! - cart: anonymous. `POST /carts` creates one, `GET /carts/{id}` reads it,
//!   `POST /carts/{id}/items` takes a quantity DELTA (negative removes, 0
//!   drops the line) and answers the line's new quantity. The site keeps the id
//!   in its `chronoguest` cookie; we keep it in the picked store's `data.cart`,
//!   so picking another store starts another cart, as on the site.
//! - next pickup slot: `GET /slots?nextAvailable=true&siteIds=<store id>`.
//! - checkout: our cart id goes into the site's `chronoguest` cookie and the
//!   store into `chronosite` (JSON both, written by js-cookie in the bundle;
//!   the server reads them to render the page), then `/cartdetail` is shown.

use super::{save_store, store, str_of, Cart, CartLine, Product, Provider, Shop, Store, Web};
use anyhow::{Context, Result};
use serde_json::{json, Value};

const HOME: &str = "https://www.chronodrive.com/";
const API: &str = "https://api.chronodrive.com/v1";
/// Any page of the API's origin will do: requests from it are same-origin.
const API_PAGE: &str = "https://api.chronodrive.com/";

pub struct Chronodrive {
    web: Web,
}

impl Chronodrive {
    pub fn open(headless: bool) -> Result<Chronodrive> {
        Ok(Chronodrive { web: Web::open(Provider::Chronodrive, "chronodrive.com", headless)? })
    }

    /// `service` as in the site's config: PRODUCTS, CARTS, SITES, CITIES...
    /// `site` is the store id, for the services bound to one.
    fn api(&mut self, service: &str, method: &str, url: &str, site: Option<&str>, body: Option<Value>) -> Result<Value> {
        let expr = format!("window.__NUXT__?.config?.public?.CHR_API_{service}_APIKEY");
        let key = self.web.browser.eval_in(HOME, &expr)?;
        let key = key.as_str().with_context(|| {
            format!("No Chronodrive API key on {HOME} (bot check page?); run '{} auth login'.", Provider::Chronodrive.cmd())
        })?;
        let mut headers = vec![("x-api-key", key), ("x-device-type", "WEB")];
        if let Some(site) = site {
            headers.extend([("x-chronodrive-site-id", site), ("x-chronodrive-site-mode", "DRIVE")]);
        }
        self.web.json(API_PAGE, method, url, &headers, body)
    }

    fn read_cart(&mut self, s: &Store, id: &str) -> Result<Value> {
        self.api("CARTS", "GET", &format!("{API}/carts/{id}"), Some(&s.id), None).with_context(|| {
            // ponytail: an expired anonymous cart is not replaced automatically,
            // so a network hiccup never swaps the user's cart for an empty one.
            format!("Cannot read Chronodrive cart {id}; '{} store <postal code> --pick {}' starts a new one.", Provider::Chronodrive.cmd(), s.id)
        })
    }

    /// "Next pickup: Lundi 19 octobre à 08h30" at store `site`.
    fn next_slot(&mut self, site: &str) -> Result<Option<String>> {
        let res = self.api("SLOTS", "GET", &format!("{API}/slots?nextAvailable=true&siteIds={site}"), None, None)?;
        let Some(start) = res["content"][0]["startDate"].as_str() else { return Ok(None) };
        // Chrome's Intl, in the stores' time zone, with the site's date format.
        let expr = format!(
            "(() => {{ const d = new Date({}), z = {{timeZone: 'Europe/Paris'}};
                const day = d.toLocaleDateString('fr-FR', {{...z, weekday: 'long', day: 'numeric', month: 'long'}});
                const time = d.toLocaleTimeString('fr-FR', {{...z, hour: '2-digit', minute: '2-digit'}});
                return day[0].toUpperCase() + day.slice(1) + ' à ' + time.replace(':', 'h'); }})()",
            json!(start)
        );
        Ok(self.web.browser.eval_in(HOME, &expr)?.as_str().map(|d| format!("Next pickup: {d}")))
    }
}

impl Shop for Chronodrive {
    fn search(&mut self, query: &str, limit: u32) -> Result<Vec<Product>> {
        let s = store(Provider::Chronodrive)?;
        let size = limit.clamp(1, 50).to_string();
        let url = url::Url::parse_with_params(&format!("{API}/products"), [("searchTerm", query), ("page", "1"), ("size", &size)])?;
        let res = self.api("PRODUCTS", "GET", url.as_str(), Some(&s.id), None)?;
        Ok(res["content"].as_array().into_iter().flatten().map(product).collect())
    }

    fn cart(&mut self) -> Result<Cart> {
        let s = store(Provider::Chronodrive)?;
        let Some(id) = s.data["cart"].as_str() else { return Ok(Cart::default()) };
        let mut cart = cart_of(&self.read_cart(&s, id)?);
        // Only a hint: a slot hiccup must not hide the cart.
        cart.note = self.next_slot(&s.id).ok().flatten();
        Ok(cart)
    }

    fn set_quantity(&mut self, id: &str, quantity: u32) -> Result<u32> {
        let mut s = store(Provider::Chronodrive)?;
        let (cart, current) = match s.data["cart"].as_str().map(String::from) {
            Some(cart) => {
                let lines = cart_of(&self.read_cart(&s, &cart)?).lines;
                (cart, lines.iter().find(|l| l.product.id == id).map(|l| l.quantity).unwrap_or(0))
            }
            None => {
                let created = self.api("CARTS", "POST", &format!("{API}/carts"), Some(&s.id), None)?;
                let cart = str_of(&created["id"]).context("Chronodrive created a cart without an id")?;
                s.data["cart"] = json!(cart);
                save_store(Provider::Chronodrive, s.clone())?;
                (cart, 0)
            }
        };
        let delta = i64::from(quantity) - i64::from(current);
        if delta == 0 {
            return Ok(current);
        }
        let body = json!({"content": [{"productId": id, "quantity": delta, "clientOrigin": "WEB|SEARCH"}], "optimizedMode": true});
        let res = self.api("CARTS", "POST", &format!("{API}/carts/{cart}/items"), Some(&s.id), Some(body))?;
        // An unknown id answers 207 with returnType UNKNOWN_PRODUCT and quantity 0.
        Ok(res["content"][0]["quantity"].as_u64().unwrap_or(0) as u32)
    }

    fn find_stores(&mut self, query: &str) -> Result<Vec<Store>> {
        let url = url::Url::parse_with_params(&format!("{API}/cities"), [("searchTerm", query)])?;
        let cities = self.api("CITIES", "GET", url.as_str(), None, None)?;
        let Some(here) = cities["content"].get(0).and_then(|c| Some((num(&c["latitude"])?, num(&c["longitude"])?))) else {
            return Ok(vec![]);
        };
        let sites = self.api("SITES", "GET", &format!("{API}/sites?filter=status:OPEN&size=100"), None, None)?;
        Ok(nearest(&sites, here))
    }

    /// The CLI never sends the account's OAuth token: signing in here only
    /// clears a bot check for the profile; the cart stays the anonymous one.
    fn login(&mut self) -> Result<()> {
        self.web.login(HOME)
    }

    fn signed_in(&mut self) -> Result<bool> {
        Ok(false)
    }

    /// Signed in on the site, the window shows the account's cart instead.
    fn checkout(&mut self) -> Result<()> {
        let s = store(Provider::Chronodrive)?;
        self.web.browser.set_cookies(site_cookies(&s))?;
        self.web.show(&format!("{HOME}cartdetail"), "Book a slot and pay")
    }
}

// ------------------------------------------------------------------ parsing

fn product(p: &Value) -> Product {
    let text = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let (labels, prices) = (&p["labels"], &p["prices"]);
    let unit_price = prices["pricePerUnitMeasure"].as_f64().map(|u| {
        format!("{} € / {}", format!("{u:.2}").replace('.', ","), p["packaging"]["unit"].as_str().unwrap_or("unité"))
    });
    // Feature "520" is the Nutri-Score letter (the site's own product page reads it there).
    let nutriscore = p["characteristics"]["features"].as_array().into_iter().flatten().find(|f| f["code"] == "520");
    Product {
        id: str_of(&p["id"]).unwrap_or_default(),
        name: text(&labels["productLabel"]).unwrap_or_default(),
        brand: text(&labels["brandLabel"]),
        price: prices["defaultPrice"].as_f64(),
        unit_price,
        packaging: text(&labels["unitQuantityLabel"]),
        nutriscore: nutriscore.and_then(|f| text(&f["value"])).map(|n| n.to_uppercase()),
        ean: p["eans"].get(0).and_then(text),
        available: p["isEligible"].as_bool().unwrap_or(false) && p["remainingStock"].as_u64().unwrap_or(0) > 0,
        in_cart: 0,
    }
}

fn cart_of(c: &Value) -> Cart {
    let lines = c["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            let quantity = i["quantity"].as_u64().unwrap_or(0) as u32;
            CartLine { product: Product { in_cart: quantity, ..product(&i["product"]) }, quantity, total: i["product"]["prices"]["totalAmount"].as_f64() }
        })
        .collect();
    Cart { lines, total: c["amounts"]["totalCartAmount"].as_f64(), note: None }
}

/// The site's `chronosite` (store) and `chronoguest` (anonymous cart id)
/// cookies, for `Browser::set_cookies`. The bundle writes their JSON with
/// js-cookie, which percent-encodes it but keeps `{}:` as they are.
// ponytail: only `"` and `,` are encoded, enough for ASCII ids. Overwriting
// `chronoguest` also drops the site's search history in the CLI's profile.
fn site_cookies(s: &Store) -> Value {
    let cookie = |name: &str, v: Value| {
        let value = v.to_string().replace('"', "%22").replace(',', "%2C");
        json!({"name": name, "value": value, "url": HOME, "sameSite": "Lax"})
    };
    let mut cookies = vec![cookie("chronosite", json!({"driveId": s.id, "id": s.id, "mode": "DRIVE"}))];
    if let Some(cart) = s.data["cart"].as_str() {
        cookies.push(cookie("chronoguest", json!({"cartId": cart})));
    }
    json!(cookies)
}

/// Coordinates come as strings.
fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

/// The 10 stores nearest to `here` (lat, lng).
fn nearest(sites: &Value, here: (f64, f64)) -> Vec<Store> {
    let mut stores: Vec<(f64, Store)> = sites["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let (loc, geo) = (&s["location"], &s["location"]["geolocation"]);
            let d = km(here, (num(&geo["latitude"])?, num(&geo["longitude"])?));
            let store = Store {
                id: str_of(&s["id"])?,
                // The location's city repeats the name, so it is left out.
                name: s["name"].as_str().unwrap_or("").trim().into(),
                info: format!("{}, {d:.1} km", loc["zipCode"].as_str().unwrap_or("")),
                data: Value::Null,
            };
            Some((d, store))
        })
        .collect();
    stores.sort_by(|a, b| a.0.total_cmp(&b.0));
    stores.into_iter().take(10).map(|(_, s)| s).collect()
}

/// Great-circle distance in km.
fn km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2) + la1.cos() * la2.cos() * ((b.1 - a.1).to_radians() / 2.0).sin().powi(2);
    12742.0 * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_answers() {
        // Trimmed from GET /v1/products?searchTerm=lait and GET /v1/carts/{id}.
        let p = json!({"id": "441443", "eans": ["3596710476817"], "isEligible": true, "remainingStock": 112,
            "labels": {"productLabel": "Lait demi-écrémé UHT bio ", "brandLabel": "AUCHAN", "unitQuantityLabel": "6 x 1 L"},
            "packaging": {"unit": "L"}, "prices": {"defaultPrice": 6.89, "pricePerUnitMeasure": 1.15},
            "characteristics": {"features": [{"code": "46", "value": true}, {"code": "520", "value": "a"}]}});
        let p = product(&p);
        assert_eq!((p.id.as_str(), p.name.as_str(), p.brand.as_deref()), ("441443", "Lait demi-écrémé UHT bio", Some("AUCHAN")));
        assert_eq!((p.price, p.unit_price.as_deref(), p.packaging.as_deref()), (Some(6.89), Some("1,15 € / L"), Some("6 x 1 L")));
        assert_eq!((p.nutriscore.as_deref(), p.ean.as_deref(), p.available), (Some("A"), Some("3596710476817"), true));

        let cart = cart_of(&json!({"id": "x", "amounts": {"totalCartAmount": 13.7}, "items": [{"quantity": 2,
            "product": {"id": "378524", "isEligible": true, "remainingStock": 0, "labels": {"productLabel": "Lait"},
                "prices": {"defaultPrice": 6.85, "totalAmount": 13.7}}}]}));
        assert_eq!((cart.total, cart.lines.len()), (Some(13.7), 1));
        let l = &cart.lines[0];
        assert_eq!((l.product.id.as_str(), l.quantity, l.product.in_cart, l.total, l.product.available), ("378524", 2, 2, Some(13.7), false));

        let sites = json!({"content": [
            {"id": "1036", "name": "Compiègne", "location": {"zipCode": "60200", "city": "Compiègne ", "geolocation": {"latitude": "49.389", "longitude": "2.7846"}}},
            {"id": "1001", "name": "Marcq en Baroeul", "location": {"zipCode": "59700", "city": "Marcq en Baroeul", "geolocation": {"latitude": "50.6824", "longitude": "3.0848"}}},
            {"id": "9999", "name": "No coordinates", "location": {}}]});
        let near = nearest(&sites, (50.6311, 3.0468)); // Lille 59000
        assert_eq!(near.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["1001", "1036"]);
        assert_eq!(near[0].info, "59700, 6.3 km");

        let s = Store { id: "1048".into(), data: json!({"cart": "260a0f62-df38"}), ..Store::default() };
        let c = site_cookies(&s);
        assert_eq!(c[0]["value"], r#"{%22driveId%22:%221048%22%2C%22id%22:%221048%22%2C%22mode%22:%22DRIVE%22}"#);
        assert_eq!((c[1]["name"].as_str(), c[1]["value"].as_str()), (Some("chronoguest"), Some(r#"{%22cartId%22:%22260a0f62-df38%22}"#)));
    }
}
