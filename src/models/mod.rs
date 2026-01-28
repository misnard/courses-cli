use serde::{Deserialize, Serialize};

// ============ Search Models ============

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub search_term: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub product_filters: Vec<ProductFilter>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct ProductFilter {
    pub key: String,
    pub value: Vec<String>,
}

// Autocomplete API response wrapper
#[derive(Debug, Serialize, Deserialize)]
pub struct AutocompleteResponse {
    pub data: AutocompleteData,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutocompleteData {
    #[serde(default)]
    pub filtered_suggestions: Vec<AutocompleteSuggestion>,
    #[serde(default)]
    pub suggestions: Vec<AutocompleteSuggestion>,
    #[serde(default)]
    pub products: Vec<Product>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutocompleteSuggestion {
    pub category: Option<String>,
    pub text: Option<String>,
    pub link: Option<String>,
}

// ============ Product Models ============

#[derive(Debug, Serialize, Deserialize)]
pub struct Product {
    #[serde(rename = "type")]
    pub product_type: Option<String>,
    pub id: String,
    pub attributes: ProductAttributes,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductAttributes {
    pub ean: Option<String>,
    pub cdbase: Option<String>,
    pub title: String,
    pub short_title: Option<String>,
    pub brand: Option<String>,
    pub slug: Option<String>,
    pub business_type: Option<String>,
    pub offer_service_id: Option<String>,
    pub uri: Option<String>,
    #[serde(default)]
    pub categories: Option<Vec<Category>>,
    pub top_category_name: Option<String>,
    pub images: Option<ProductImages>,
    pub packaging: Option<String>,
    pub format: Option<String>,
    pub origin: Option<String>,
    pub nutriscore: Option<Nutriscore>,
    pub customer_reviews: Option<CustomerReviews>,
    #[serde(default)]
    pub offers: serde_json::Value, // Complex nested structure
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductImages {
    #[serde(rename = "0")]
    pub primary: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub label: String,
    pub slug: String,
    pub uri: String,
    pub level: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nutriscore {
    pub value: Option<String>,
    pub is_calculated: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerReviews {
    pub average: Option<f64>,
    pub count: Option<u32>,
}

// ============ Price Models ============

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub price: f64,
    pub tax_message: Option<String>,
    pub unit_of_measure: Option<String>,
    pub per_unit: Option<f64>,
    pub per_unit_label: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Availability {
    pub purchasable: bool,
    pub stopped: Option<bool>,
    pub suspended: Option<bool>,
    pub has_low_quantity: Option<bool>,
}

// ============ Cart Models ============

#[derive(Debug, Serialize, Deserialize)]
pub struct CartResponse {
    pub cart: Cart,
    #[serde(default)]
    pub marketplace: Option<serde_json::Value>,
    #[serde(rename = "endPoints")]
    pub endpoints: Option<CartEndpoints>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cart {
    #[serde(rename = "cartID")]
    pub cart_id: String,
    pub total_amount: f64,
    pub total_price: f64,
    pub total_fees: f64,
    pub total_discount: Option<f64>,
    pub total_savings: Option<f64>,
    pub nb_products: u32,
    pub currency: String,
    #[serde(rename = "type")]
    pub cart_type: Option<String>,
    pub facility_service_id: Option<String>,
    #[serde(default)]
    pub items: Vec<CartCategory>,
    pub slot: Option<CartSlot>,
    pub returnable_bags: Option<ReturnableBags>,
    pub is_slot_available: Option<bool>,
    pub is_slot_booked: Option<bool>,
    pub has_loyalty_card: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CartCategory {
    pub category: String,
    #[serde(default)]
    pub products: Vec<CartProduct>,
    #[serde(default)]
    pub discounts: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartProduct {
    pub counter: u32,
    pub total_item_price: f64,
    pub total_item_price_without_promo: Option<f64>,
    pub total_item_discount: Option<f64>,
    pub available: bool,
    pub product: Product,
    pub is_non_food: Option<bool>,
    pub url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartSlot {
    #[serde(rename = "ref")]
    pub slot_ref: Option<String>,
    pub hour: Option<String>,
    pub date: Option<SlotDate>,
    pub is_booked: Option<bool>,
    pub is_available: Option<bool>,
    pub slice: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SlotDate {
    pub begin: Option<String>,
    pub end: Option<String>,
    pub cutoff: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnableBags {
    pub quantity: u32,
    pub unit_price: f64,
    pub total_price: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CartEndpoints {
    pub cart: Option<String>,
    pub product: Option<String>,
    pub slot: Option<String>,
    pub timeslots: Option<String>,
}

// ============ User/Me Models ============

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub session_id: Option<String>,
    pub gender: Option<String>,
    pub last_name: Option<String>,
    pub first_name: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub loyalty_card: Option<LoyaltyCard>,
    pub slot: Option<UserSlot>,
    pub user_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoyaltyCard {
    pub number: String,
    #[serde(rename = "type")]
    pub card_type: String,
    pub is_secured: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSlot {
    pub title: Option<String>,
    pub hour: Option<String>,
    #[serde(rename = "ref")]
    pub slot_ref: Option<String>,
    pub date: Option<SlotDate>,
}

// ============ FlareSolverr Models ============

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct FlareSolverrRequest {
    pub cmd: String,
    pub url: String,
    #[serde(rename = "maxTimeout")]
    pub max_timeout: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FlareSolverrResponse {
    pub status: String,
    pub message: String,
    pub solution: Option<FlareSolverrSolution>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlareSolverrSolution {
    pub url: String,
    pub status: u16,
    pub cookies: Vec<FlareSolverrCookie>,
    pub user_agent: String,
    pub response: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlareSolverrCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: Option<f64>,
    pub http_only: bool,
    pub secure: bool,
    pub same_site: Option<String>,
}

// ============ Timeslot Models ============

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeslotRequest {
    pub slot_ref: String,
    pub origin: String,
    pub facility_service_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeslotsResponse {
    #[serde(default)]
    pub timeslots: Vec<TimeslotDay>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeslotDay {
    pub day: Option<String>,
    pub date: Option<String>,
    pub day_slot: Option<String>,
    pub available_day: Option<bool>,
    #[serde(default)]
    pub cells: Vec<Timeslot>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeslot {
    #[serde(rename = "ref")]
    pub slot_ref: String,
    pub hour_start: Option<String>,
    pub hour_end: Option<String>,
    pub fees: Option<f64>,
    pub available: Option<bool>,
    pub selected: Option<bool>,
    pub fill_rate: Option<u32>,
    pub facility_service_id: Option<String>,
}
