use crate::browser::Response;
use crate::error::CarrefourError;
use crate::models::{AutocompleteResponse, CartResponse, TimeslotsResponse, UserProfile};
use crate::session::Session;
use crate::spec::{Body, Request};
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::sync::Mutex;

const BASE_URL: &str = "https://www.carrefour.fr/api";

/// Every call runs as a `fetch` inside a real Chrome page (see `browser.rs`):
/// Cloudflare challenges TLS-impersonating clients like rquest after a few
/// requests, but lets Chrome through. The session renews itself from the SSO
/// cookie when Carrefour answers 401/403.
pub struct CarrefourClient {
    session: Mutex<Session>,
}

impl CarrefourClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            session: Mutex::new(Session::open()?),
        })
    }

    /// Raw request, used by the generic `api` command.
    pub fn send(&self, req: &Request) -> Result<Response> {
        self.session.lock().unwrap().send(req)
    }

    /// Renew the storefront session; fails when the SSO session is dead.
    /// (ForgeRock's session API is blocked by Carrefour's proxy, so this is
    /// the only way to tell, and it resets the 60 min idle timer as a bonus.)
    pub fn refresh(&self) -> Result<()> {
        self.session.lock().unwrap().refresh()
    }

    fn call<T: DeserializeOwned>(&self, method: &str, url: &str, body: Option<Value>) -> Result<T> {
        let mut headers = serde_json::Map::new();
        headers.insert("x-requested-with".into(), json!("XMLHttpRequest"));
        if body.is_some() {
            headers.insert("content-type".into(), json!("application/json"));
        }
        let req = Request {
            method: method.into(),
            url: url.into(),
            headers,
            body: body.map(|b| Body::Text(b.to_string())),
        };
        let res = self.send(&req)?;
        if !(200..300).contains(&res.status) {
            return Err(CarrefourError::from_status(res.status, &res.text).into());
        }
        let text = if res.text.trim().is_empty() { "null" } else { &res.text };
        serde_json::from_str(text).with_context(|| format!("Failed to parse response from {}", url))
    }

    /// Get user profile information
    pub async fn get_me(&self) -> Result<UserProfile> {
        self.call("GET", &format!("{}/me", BASE_URL), None)
    }

    /// Search for products using autocomplete API
    pub async fn search(&self, query: &str, limit: u32) -> Result<AutocompleteResponse> {
        let url = url::Url::parse_with_params(
            "https://www.carrefour.fr/autocomplete",
            [("q", query), ("maxResults", &limit.to_string()), ("productCard", "true")],
        )?;
        self.call("GET", url.as_str(), None)
    }

    /// Get current cart
    pub async fn get_cart(&self) -> Result<CartResponse> {
        self.call("GET", &format!("{}/cart", BASE_URL), None)
    }

    /// (basketServiceId, subBasketType) of the selected store, required by cart writes.
    async fn basket(&self) -> Result<(String, String)> {
        let cart = self.get_cart().await?.cart;
        let id = cart
            .facility_service_id
            .ok_or_else(|| anyhow::anyhow!("No store selected. Please select a store first."))?;
        Ok((id, cart.cart_type.unwrap_or_else(|| "drive_clcv".to_string())))
    }

    fn patch_item(&self, url: &str, ean: &str, body: Value) -> Result<()> {
        self.call::<Value>("PATCH", url, Some(body)).map(|_| ()).map_err(|e| {
            match e.downcast_ref::<CarrefourError>() {
                Some(CarrefourError::NotFound(_)) => CarrefourError::ProductNotFound(ean.to_string()).into(),
                _ => e,
            }
        })
    }

    /// Add product to cart by EAN
    pub async fn add_to_cart(&self, ean: &str, quantity: u32) -> Result<()> {
        let (basket_service_id, sub_basket_type) = self.basket().await?;
        let body = json!({
            "trackingRequest": { "pageType": "search", "pageId": "search" },
            "items": [{
                "basketServiceId": basket_service_id,
                "counter": quantity,
                "ean": ean,
                "subBasketType": sub_basket_type
            }]
        });
        self.patch_item(&format!("{}/cart", BASE_URL), ean, body)
    }

    /// Remove product from cart by EAN
    pub async fn remove_from_cart(&self, ean: &str) -> Result<()> {
        let (basket_service_id, sub_basket_type) = self.basket().await?;
        // Remove uses /api/cart/items endpoint with flat body
        let body = json!({
            "trackingRequest": { "pageType": "basket", "pageId": "basket" },
            "basketServiceId": basket_service_id,
            "ean": ean,
            "subBasketType": sub_basket_type
        });
        self.patch_item(&format!("{}/cart/items", BASE_URL), ean, body)
    }

    /// Update product quantity in cart (use counter=0 to remove)
    pub async fn update_cart_quantity(&self, ean: &str, quantity: u32) -> Result<()> {
        let (basket_service_id, sub_basket_type) = self.basket().await?;
        let body = json!({
            "trackingRequest": { "pageType": "basket", "pageId": "basket" },
            "items": [{
                "basketServiceId": basket_service_id,
                "counter": quantity,
                "ean": ean,
                "subBasketType": sub_basket_type
            }]
        });
        // Use ?refresh=true and pageType "basket" for cart modifications
        self.patch_item(&format!("{}/cart?refresh=true", BASE_URL), ean, body)
    }

    /// Get available timeslots for delivery/pickup
    pub async fn get_timeslots(&self, facility_service_id: Option<&str>) -> Result<TimeslotsResponse> {
        let fsid = match facility_service_id {
            Some(id) => id.to_string(),
            None => self.basket().await?.0,
        };
        self.call("GET", &format!("{}/timeslots?facilityServiceId={}", BASE_URL, fsid), None)
    }

    /// Select a timeslot for the cart
    pub async fn select_slot(&self, slot_ref: &str) -> Result<()> {
        let url = format!("{}/cart/slot", BASE_URL);
        self.call::<Value>("PUT", &url, Some(json!({ "slotRef": slot_ref }))).map(|_| ())
    }
}
