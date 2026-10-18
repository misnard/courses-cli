//! Carrefour, through `CarrefourClient` (session, SSO renewal and models live
//! in `client.rs`, `session.rs` and `models`). Its own extras (slots, `api`,
//! cookie import, `auth serve`) are Carrefour-only commands.

use super::{Cart, CartLine, Product, Shop};
use crate::client::CarrefourClient;
use crate::models;
use anyhow::Result;
use serde_json::Value;

pub struct Carrefour {
    client: CarrefourClient,
}

impl Carrefour {
    pub fn open() -> Result<Carrefour> {
        Ok(Carrefour { client: CarrefourClient::new()? })
    }
}

/// offers → {ean} → {first offer id} → attributes
fn offer(p: &models::Product) -> &Value {
    let offers = p.attributes.ean.as_ref().and_then(|ean| p.attributes.offers[ean].as_object());
    offers.and_then(|o| o.values().next()).map_or(&Value::Null, |o| &o["attributes"])
}

fn product(p: &models::Product) -> Product {
    let a = &p.attributes;
    let offer = offer(p);
    Product {
        id: a.ean.clone().unwrap_or_else(|| p.id.clone()),
        name: a.title.clone(),
        brand: a.brand.clone(),
        price: offer["price"]["price"].as_f64(),
        unit_price: offer["price"]["perUnitLabel"].as_str().map(String::from),
        packaging: a.packaging.clone(),
        nutriscore: a.nutriscore.as_ref().and_then(|n| n.value.clone()),
        ean: a.ean.clone(),
        available: offer["availability"]["purchasable"].as_bool().unwrap_or(false),
        in_cart: 0,
    }
}

impl Shop for Carrefour {
    fn search(&mut self, query: &str, limit: u32) -> Result<Vec<Product>> {
        Ok(self.client.search(query, limit)?.data.products.iter().map(product).collect())
    }

    fn cart(&mut self) -> Result<Cart> {
        let cart = self.client.get_cart()?.cart;
        let lines = cart
            .items
            .iter()
            .flat_map(|c| &c.products)
            .map(|i| CartLine {
                product: Product { available: i.available, ..product(&i.product) },
                quantity: i.counter,
                total: Some(i.total_item_price),
            })
            .collect();
        Ok(Cart {
            lines,
            total: Some(cart.total_amount),
            note: cart.slot.and_then(|s| s.hour).map(|h| format!("Pickup slot: {h}")),
        })
    }

    fn set_quantity(&mut self, id: &str, quantity: u32) -> Result<u32> {
        match quantity {
            0 => self.client.remove_from_cart(id)?,
            // `counter` is the target quantity, not a delta.
            q => self.client.add_to_cart(id, q)?,
        }
        Ok(quantity)
    }

    fn signed_in(&mut self) -> Result<bool> {
        let me = self.client.get_me()?;
        Ok(me.email.is_some() || me.user_id.is_some())
    }
}
