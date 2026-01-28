use crate::config::Config;
use crate::error::CarrefourError;
use crate::models::{AutocompleteResponse, CartResponse, TimeslotsResponse, UserProfile};
use anyhow::{Context, Result};
use rquest::header::{HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT};
use rquest::Impersonate;

const BASE_URL: &str = "https://www.carrefour.fr/api";

pub struct CarrefourClient {
    client: rquest::Client,
    cookies: String,
    #[allow(dead_code)]
    user_agent: Option<String>,
}

impl CarrefourClient {
    pub fn new() -> Result<Self> {
        let config = Config::load()?;

        // Use full cookies string if available, otherwise just session_id
        let cookies = if let Some(cookies) = config.cookies {
            cookies
        } else if let Some(sid) = config.session_id {
            format!("FRONTONE_SESSID={}", sid)
        } else {
            return Err(CarrefourError::NotAuthenticated.into());
        };

        let user_agent = config.user_agent.clone();

        let mut headers = HeaderMap::new();
        // Don't set Content-Type as default - only for POST/PATCH/PUT with json body
        headers.insert(ORIGIN, HeaderValue::from_static("https://www.carrefour.fr"));
        headers.insert(
            "x-requested-with",
            HeaderValue::from_static("XMLHttpRequest"),
        );
        headers.insert(
            "accept",
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(
            "accept-language",
            HeaderValue::from_static("en-US,en;q=0.9"),
        );
        headers.insert("cache-control", HeaderValue::from_static("no-cache"));
        headers.insert("pragma", HeaderValue::from_static("no-cache"));

        // If we have a user agent from FlareSolverr, use it and skip impersonation
        // to avoid TLS fingerprint mismatch
        let client = if let Some(ref ua) = user_agent {
            if let Ok(ua_value) = HeaderValue::from_str(ua) {
                headers.insert(USER_AGENT, ua_value);
            }
            // Use plain client without impersonation when we have FlareSolverr cookies
            rquest::Client::builder()
                .default_headers(headers)
                .cookie_store(true)
                .build()?
        } else {
            // Use Chrome impersonation when no FlareSolverr session
            rquest::Client::builder()
                .impersonate(Impersonate::Chrome131)
                .default_headers(headers)
                .cookie_store(true)
                .build()?
        };

        Ok(Self {
            client,
            cookies,
            user_agent,
        })
    }

    fn cookies(&self) -> &str {
        &self.cookies
    }

    fn handle_error(status: rquest::StatusCode, body: &str) -> CarrefourError {
        CarrefourError::from_status(status.as_u16(), body)
    }

    /// Get user profile information
    pub async fn get_me(&self) -> Result<UserProfile> {
        let url = format!("{}/me", BASE_URL);

        let response = self
            .client
            .get(&url)
            .header("cookie", self.cookies())
            .header(REFERER, "https://www.carrefour.fr/")
            .send()
            .await
            .context("Failed to fetch user profile")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Self::handle_error(status, &body).into());
        }

        response
            .json::<UserProfile>()
            .await
            .context("Failed to parse user profile response")
    }

    /// Search for products using autocomplete API
    pub async fn search(&self, query: &str, limit: u32) -> Result<AutocompleteResponse> {
        let url = format!(
            "https://www.carrefour.fr/autocomplete?q={}&maxResults={}&productCard=true",
            urlencoding::encode(query),
            limit
        );

        let referer = format!(
            "https://www.carrefour.fr/s?q={}",
            urlencoding::encode(query)
        );

        let response = self
            .client
            .get(&url)
            .header(REFERER, referer)
            .header("cookie", self.cookies())
            .send()
            .await
            .context("Failed to send search request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Self::handle_error(status, &body).into());
        }

        response
            .json::<AutocompleteResponse>()
            .await
            .context("Failed to parse search response")
    }

    /// Get current cart
    pub async fn get_cart(&self) -> Result<CartResponse> {
        let url = format!("{}/cart", BASE_URL);

        let response = self
            .client
            .get(&url)
            .header("cookie", self.cookies())
            .header(REFERER, "https://www.carrefour.fr/cart/driveclcv")
            .send()
            .await
            .context("Failed to fetch cart")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Self::handle_error(status, &body).into());
        }

        response
            .json::<CartResponse>()
            .await
            .context("Failed to parse cart response")
    }

    /// Add product to cart by EAN
    pub async fn add_to_cart(&self, ean: &str, quantity: u32) -> Result<()> {
        // First get cart to find facilityServiceId and cart type
        let cart_response = self.get_cart().await?;
        let cart = &cart_response.cart;

        let basket_service_id = cart
            .facility_service_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No store selected. Please select a store first."))?;

        let sub_basket_type = cart
            .cart_type
            .clone()
            .unwrap_or_else(|| "drive_clcv".to_string());

        let url = format!("{}/cart", BASE_URL);

        let body = serde_json::json!({
            "trackingRequest": {
                "pageType": "search",
                "pageId": "search"
            },
            "items": [{
                "basketServiceId": basket_service_id,
                "counter": quantity,
                "ean": ean,
                "subBasketType": sub_basket_type
            }]
        });

        let response = self
            .client
            .patch(&url)
            .header("cookie", self.cookies())
            .header(REFERER, "https://www.carrefour.fr/")
            .json(&body)
            .send()
            .await
            .context("Failed to add item to cart")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status.as_u16() == 404 {
                return Err(CarrefourError::ProductNotFound(ean.to_string()).into());
            }
            return Err(Self::handle_error(status, &body).into());
        }

        Ok(())
    }

    /// Remove product from cart by EAN
    pub async fn remove_from_cart(&self, ean: &str) -> Result<()> {
        // Get cart to find facilityServiceId and cart type
        let cart_response = self.get_cart().await?;
        let cart = &cart_response.cart;

        let basket_service_id = cart
            .facility_service_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No store selected. Please select a store first."))?;

        let sub_basket_type = cart
            .cart_type
            .clone()
            .unwrap_or_else(|| "drive_clcv".to_string());

        // Remove uses /api/cart/items endpoint with flat body
        let url = format!("{}/cart/items", BASE_URL);

        let body = serde_json::json!({
            "trackingRequest": {
                "pageType": "basket",
                "pageId": "basket"
            },
            "basketServiceId": basket_service_id,
            "ean": ean,
            "subBasketType": sub_basket_type
        });

        let response = self
            .client
            .patch(&url)
            .header("cookie", self.cookies())
            .header(REFERER, "https://www.carrefour.fr/cart/driveclcv")
            .json(&body)
            .send()
            .await
            .context("Failed to remove item from cart")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status.as_u16() == 404 {
                return Err(CarrefourError::ProductNotFound(ean.to_string()).into());
            }
            return Err(Self::handle_error(status, &body).into());
        }

        Ok(())
    }

    /// Update product quantity in cart (use counter=0 to remove)
    pub async fn update_cart_quantity(&self, ean: &str, quantity: u32) -> Result<()> {
        // Get cart to find facilityServiceId and cart type
        let cart_response = self.get_cart().await?;
        let cart = &cart_response.cart;

        let basket_service_id = cart
            .facility_service_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No store selected. Please select a store first."))?;

        let sub_basket_type = cart
            .cart_type
            .clone()
            .unwrap_or_else(|| "drive_clcv".to_string());

        // Use ?refresh=true and pageType "basket" for cart modifications
        let url = format!("{}/cart?refresh=true", BASE_URL);

        let body = serde_json::json!({
            "trackingRequest": {
                "pageType": "basket",
                "pageId": "basket"
            },
            "items": [{
                "basketServiceId": basket_service_id,
                "counter": quantity,
                "ean": ean,
                "subBasketType": sub_basket_type
            }]
        });

        let response = self
            .client
            .patch(&url)
            .header("cookie", self.cookies())
            .header(REFERER, "https://www.carrefour.fr/cart/driveclcv")
            .json(&body)
            .send()
            .await
            .context("Failed to update cart quantity")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status.as_u16() == 404 {
                return Err(CarrefourError::ProductNotFound(ean.to_string()).into());
            }
            return Err(Self::handle_error(status, &body).into());
        }

        Ok(())
    }

    /// Get available timeslots for delivery/pickup
    pub async fn get_timeslots(
        &self,
        facility_service_id: Option<&str>,
    ) -> Result<TimeslotsResponse> {
        // First get cart to find facility_service_id if not provided
        let fsid = match facility_service_id {
            Some(id) => id.to_string(),
            None => {
                let cart = self.get_cart().await?;
                cart.cart.facility_service_id.ok_or_else(|| {
                    anyhow::anyhow!("No store selected. Please select a store first.")
                })?
            }
        };

        let url = format!("{}/timeslots?facilityServiceId={}", BASE_URL, fsid);

        let response = self
            .client
            .get(&url)
            .header("cookie", self.cookies())
            .header(REFERER, "https://www.carrefour.fr/cart/driveclcv")
            .send()
            .await
            .context("Failed to fetch timeslots")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Self::handle_error(status, &body).into());
        }

        response
            .json::<TimeslotsResponse>()
            .await
            .context("Failed to parse timeslots response")
    }

    /// Select a timeslot for the cart
    pub async fn select_slot(&self, slot_ref: &str) -> Result<()> {
        let url = format!("{}/cart/slot", BASE_URL);

        let body = serde_json::json!({
            "slotRef": slot_ref
        });

        let response = self
            .client
            .put(&url)
            .header("cookie", self.cookies())
            .header(REFERER, "https://www.carrefour.fr/cart/driveclcv")
            .json(&body)
            .send()
            .await
            .context("Failed to select slot")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Self::handle_error(status, &body).into());
        }

        Ok(())
    }
}
