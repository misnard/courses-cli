//! Casino proximité (mescoursesdeproximite.com): Petit Casino, Casino Shop,
//! Spar, Vival... Search, cart and cart writes from
//! https://github.com/ncleton/casino-proximite-mcp (MIT); the store finder
//! from the site's own route table (`/js/routing`) and front-end scripts.
//!
//! No bot protection answers (plain HTTP gets a 200); requests still leave
//! from the store's page, which also makes it the session's store. Pages are
//! Symfony/Twig HTML, read with plain string search:
//! - stores: POST `/localisation` {search} → `[{id: "ville|31680"}]`, then
//!   POST `/resultat-localisation` {loc, ajax: true} → `{html}` listing stores;
//! - search: GET `/recherche/{store code}?produit_recherche=`;
//! - cart: the cart panel of the store page (`ligne-panier` blocks);
//! - writes: POST `/commande/ajouter-panier` {idProduit, qte}, `qte` being the
//!   target (0 removes); it answers `{success, html}`, html the new cart panel,
//!   or an HTTP 500 for an unknown id;
//! - next pickup slot: the store page's header ("Retrait en magasin", the same
//!   data `/modale/creneaux` lists in full);
//! - checkout: `/cde/{store code}/1`, where the cart panel's "Valider mon
//!   panier" leads.
//!
//! The anonymous cart lives in the COOKIECDE / COOKIECDEMAG cookies (90 days),
//! so `Web`'s cookie jar keeps it across runs.

use super::{parse_euros, store, Cart, CartLine, Product, Provider, Shop, Store, Web};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

const SITE: &str = "https://www.mescoursesdeproximite.com";
/// Marks the element holding a product's cart id and quantity, in search
/// cards and cart lines alike.
const BUTTON: &str = "container-btn-ajouter-au-panier";

pub struct Casino {
    web: Web,
}

/// The store's page, e.g. `.../courses-en-ligne/casino-shop-93210/TL324`.
fn page(s: &Store) -> String {
    format!("{SITE}/courses-en-ligne/{}", s.data["path"].as_str().unwrap_or(""))
}

impl Casino {
    pub fn open(headless: bool) -> Result<Casino> {
        Ok(Casino { web: Web::open(Provider::Casino, "mescoursesdeproximite.com", headless)? })
    }

    /// A form POST sent from `from`, answering JSON.
    fn post(&mut self, from: &str, path: &str, form: &[(&str, &str)]) -> Result<Value> {
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let headers = [
            ("content-type", "application/x-www-form-urlencoded; charset=UTF-8"),
            ("x-requested-with", "XMLHttpRequest"),
            ("accept", "application/json"),
        ];
        let text = self.web.send(from, "POST", &format!("{SITE}{path}"), &headers, Some(body))?;
        serde_json::from_str(&text).with_context(|| format!("Unexpected answer from {path}: {}", text.chars().take(120).collect::<String>()))
    }

    fn get(&mut self, from: &str, url: &str) -> Result<String> {
        self.web.send(from, "GET", url, &[], None)
    }
}

impl Shop for Casino {
    /// The site answers every match at once (~200 for "lait").
    fn search(&mut self, query: &str, _limit: u32) -> Result<Vec<Product>> {
        let s = store(Provider::Casino)?;
        let url = url::Url::parse_with_params(&format!("{SITE}/recherche/{}", s.id), [("produit_recherche", query)])?;
        let html = self.get(&page(&s), url.as_str())?;
        Ok(html.split("class=\"vignette-produit ").skip(1).filter_map(product).collect())
    }

    fn cart(&mut self) -> Result<Cart> {
        let p = page(&store(Provider::Casino)?);
        let html = self.get(&p, &p)?;
        let total = html.find("Total commande").and_then(|i| html[i..].split('<').next()).and_then(parse_euros);
        Ok(Cart { lines: lines(&html), total, note: next_pickup(&html) })
    }

    fn set_quantity(&mut self, id: &str, quantity: u32) -> Result<u32> {
        let p = page(&store(Provider::Casino)?);
        let qte = quantity.to_string();
        let form = [("idProduit", id), ("qte", qte.as_str()), ("reloadCommande", "false"), ("famille", ""), ("sousfamille", "")];
        let r = match self.post(&p, "/commande/ajouter-panier", &form) {
            // The site's only answer to an unknown id.
            Err(e) if e.to_string().starts_with("HTTP 500 ") => bail!("Unknown Casino product id {id}: use an ID that search prints."),
            r => r?,
        };
        if r["success"] != true {
            bail!("Casino refused to change product {id}.");
        }
        let held = lines(r["html"].as_str().unwrap_or("")).into_iter().find(|l| l.product.id == id);
        Ok(held.map_or(0, |l| l.quantity))
    }

    fn find_stores(&mut self, query: &str) -> Result<Vec<Store>> {
        let home = format!("{SITE}/");
        // Towns and postal codes matching `query`, best first.
        let places = self.post(&home, "/localisation", &[("search", query)])?;
        let Some(loc) = places[0]["id"].as_str() else { return Ok(vec![]) };
        let r = self.post(&home, "/resultat-localisation", &[("loc", loc), ("slug", ""), ("ajax", "true")])?;
        Ok(stores(r["html"].as_str().unwrap_or("")))
    }

    fn checkout(&mut self) -> Result<()> {
        let s = store(Provider::Casino)?;
        self.web.show(&format!("{SITE}/cde/{}/1", s.id), "Book a slot and pay")
    }

    fn login(&mut self) -> Result<()> {
        self.web.login(&format!("{SITE}/compte/"))
    }

    /// Heuristic from the reference project: a signed-in header offers to
    /// sign out; an anonymous one says "Me connecter".
    fn signed_in(&mut self) -> Result<bool> {
        let home = format!("{SITE}/");
        let html = self.get(&home, &home)?.to_lowercase();
        Ok(html.contains("déconnexion") || html.contains("se déconnecter"))
    }
}

// ------------------------------------------------------------------ parsing

/// A search card. Its cart button carries the id `cart add` takes, which is
/// not the favourites button's `data-produit` nor the article code in the URL.
// ponytail: a card without a cart button (never seen) is dropped, not shown
// as out of stock.
fn product(card: &str) -> Option<Product> {
    let (head, button) = card.split_at(card.find(BUTTON)?);
    Some(Product {
        id: attr(button, "data-produit")?,
        name: text(head, "<h3")?,
        brand: attr(head, "data-marque"),
        price: text(head, "text-nowrap").as_deref().and_then(parse_euros),
        unit_price: text(head, "montserratlight"),
        available: true,
        in_cart: attr(button, "data-qte").and_then(|q| q.parse().ok()).unwrap_or(0),
        ..Default::default()
    })
}

/// Cart lines of a page or of the cart panel a write answers.
fn lines(html: &str) -> Vec<CartLine> {
    html.split("class=\"ligne-panier\"")
        .skip(1)
        .filter_map(|l| {
            let (head, button) = l.split_at(l.find(BUTTON)?);
            let quantity = attr(button, "data-qte")?.parse().ok()?;
            Some(CartLine {
                product: Product {
                    id: attr(button, "data-produit")?,
                    brand: text(head, "<b>"),
                    // The paragraph after the bold brand.
                    name: text(&head[head.find("</b>")?..], "<p")?,
                    available: true,
                    in_cart: quantity,
                    ..Default::default()
                },
                quantity,
                total: text(l, "text-nowrap").as_deref().and_then(parse_euros),
            })
        })
        .collect()
}

/// The header's pickup button: "Retrait en magasin <br> <span>19/10/2026 à 8h</span>".
fn next_pickup(html: &str) -> Option<String> {
    let slot = between(html, "Retrait en magasin <br", "</span>")?.rsplit('>').next()?.trim();
    (!slot.is_empty()).then(|| format!("Next pickup: {}", unescape(slot)))
}

/// Stores of a `/resultat-localisation` answer, nearest first (the site ranks
/// them its own way); those without a "Choisir" link (no online service) are
/// left out.
fn stores(html: &str) -> Vec<Store> {
    let mut found: Vec<(f64, Store)> = html
        .split("resultat-un-magasin js-magasin")
        .skip(1)
        .filter_map(|b| {
            let path = between(b, "href=\"/courses-en-ligne/", "\"")?;
            let distance = text(b, "class=\"distance").unwrap_or_default();
            let km = distance.split(' ').next().and_then(|d| d.parse().ok()).unwrap_or(f64::MAX);
            let services: Vec<String> = attr(b, "data-service").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            let town = b.find("<br").and_then(|i| text(&b[i..], "<br")).unwrap_or_default();
            let mut info = vec![distance, town.replace(',', "")];
            info.extend(services.into_iter().filter(|s| s != "tout"));
            let store = Store {
                id: path.rsplit('/').next()?.into(),
                name: text(b, "nom-magasin")?,
                info: info.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "),
                data: json!({"path": path}),
            };
            Some((km, store))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, s)| s).collect()
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let rest = &s[s.find(start)? + start.len()..];
    Some(&rest[..rest.find(end)?])
}

/// The first `name="..."` attribute value in `s`.
fn attr(s: &str, name: &str) -> Option<String> {
    between(s, &format!(" {name}=\""), "\"").map(unescape)
}

/// The first non-blank text after `marker`, which sits inside a tag.
fn text(s: &str, marker: &str) -> Option<String> {
    let rest = &s[s.find(marker)?..];
    rest.split('>').skip(1).map(|t| t.split('<').next().unwrap_or("").trim()).find(|t| !t.is_empty()).map(unescape)
}

/// Twig escapes with htmlspecialchars, which only emits these five entities.
fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&#039;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pages() {
        let search = r#"<div class="vignette-produit col-lg-3 py-3" data-promo="0" data-marque="Régilait">
            <div class="btn-produit-favori" data-produit="1851"></div>
            <div class="produit-desc"><a href="/produit/regilait-lait/TL324/226956">
              <h3 class="d-inline"><strong>Régilait Lait d&#039;Été 300g</strong></h3><br/>
              <span class="montserratlight">13,30 €/KILO</span></a></div>
            <div><span class="montserratsemibold fontSize15-15 text-nowrap mr-3 ">3.99 €</span></div>
            <div class="container-btn-ajouter-au-panier" data-qte="2" data-produit="10604230" data-etat-ajax="0">
              <button data-qte="3" data-action="plus"></button></div></div>"#;
        let p: Vec<Product> = search.split("class=\"vignette-produit ").skip(1).filter_map(product).collect();
        assert_eq!((p[0].id.as_str(), p[0].name.as_str(), p[0].brand.as_deref()), ("10604230", "Régilait Lait d'Été 300g", Some("Régilait")));
        assert_eq!((p[0].price, p[0].unit_price.as_deref(), p[0].in_cart), (Some(3.99), Some("13,30 €/KILO"), 2));

        let cart = r#"<div class="container-lignes-panier"><div class="ligne-panier"><div class="infos"><a href="/produit/x/TL324/226956">
              <p class="m-0"><b> Régilait </b></p><p class="m-0"> Régilait Lait En Poudre 300g </p><p class="m-0">13,30 €/KILO</p></a>
              <p class="m-0  montserratsemibold fontSize15-15  text-nowrap"> 7.98 € </p></div>
            <div class="container-btn-ajouter-au-panier" data-qte="2" data-produit="10604230"><button data-qte="3"></button></div>
            </div></div><div class="total-panier"><b> Total commande : 7,98 € </b></div>"#;
        let header = r#"<span class="text">Retrait en magasin <br class="d-none d-lg-flex">
            <span class="montserratlight fontSize12-12">19/10/2026 à 8h</span></span>
            <span class="text text-center montserratsemibold fontSize15-12">Retrait en magasin</span>"#;
        assert_eq!(next_pickup(header).as_deref(), Some("Next pickup: 19/10/2026 à 8h"));
        assert_eq!(next_pickup(cart), None);
        let l = lines(cart);
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].product.id.as_str(), l[0].product.name.as_str(), l[0].quantity, l[0].total), ("10604230", "Régilait Lait En Poudre 300g", 2, Some(7.98)));

        let list = r#"<div class=" resultat-un-magasin js-magasin-a-filtrer-par-service"
              data-service="[&quot;tout&quot;,&quot;retrait&quot;,&quot;livraison&quot;]" data-marker="{&quot;id&quot;:1768}">
              <div class="nom-magasin"><p> ST DENIS - AV. DU STADE CASINO SHOP </p></div>
              <div class="distance"><span class="icon-position"></span><p> 6.35 km </p></div>
              <div class="adresse"><p> 5 AVENUE DU STADE <br> 93210, St Denis </p></div>
              <a href="/courses-en-ligne/casino-shop-93210/TL324"><button>Choisir</button></a></div>
            <div class="border-top resultat-un-magasin js-magasin-a-filtrer-par-service" data-service="[&quot;tout&quot;]">
              <div class="nom-magasin"><p> ST ANDRE SPAR </p></div><div class="bulle"><p>Aucun créneau disponible</p></div></div>
            <div class="border-top resultat-un-magasin js-magasin-a-filtrer-par-service" data-service="[&quot;tout&quot;]">
              <div class="nom-magasin"><p> NEAR VIVAL </p></div><div class="distance"><p> 1.2 km </p></div>
              <a href="/courses-en-ligne/vival-91270/E7714">Choisir</a></div>"#;
        let s = stores(list);
        assert_eq!(s.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["E7714", "TL324"]);
        assert_eq!((s[1].id.as_str(), s[1].name.as_str()), ("TL324", "ST DENIS - AV. DU STADE CASINO SHOP"));
        assert_eq!((s[1].info.as_str(), &s[1].data["path"]), ("6.35 km, 93210 St Denis, retrait, livraison", &json!("casino-shop-93210/TL324")));
    }
}
