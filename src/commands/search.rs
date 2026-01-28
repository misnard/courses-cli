use crate::client::CarrefourClient;
use crate::models::Product;
use anyhow::Result;
use colored::Colorize;

/// Extract price from the nested offers structure
fn get_product_price(product: &Product) -> Option<f64> {
    let offers = &product.attributes.offers;
    // Navigate: offers -> {ean} -> {first_offer_id} -> attributes -> price -> price
    if let Some(ean) = &product.attributes.ean {
        if let Some(ean_offers) = offers.get(ean) {
            if let Some(obj) = ean_offers.as_object() {
                if let Some((_, offer)) = obj.iter().next() {
                    return offer
                        .get("attributes")
                        .and_then(|a| a.get("price"))
                        .and_then(|p| p.get("price"))
                        .and_then(|v| v.as_f64());
                }
            }
        }
    }
    None
}

/// Extract per-unit label from the nested offers structure
fn get_per_unit_label(product: &Product) -> Option<String> {
    let offers = &product.attributes.offers;
    if let Some(ean) = &product.attributes.ean {
        if let Some(ean_offers) = offers.get(ean) {
            if let Some(obj) = ean_offers.as_object() {
                if let Some((_, offer)) = obj.iter().next() {
                    return offer
                        .get("attributes")
                        .and_then(|a| a.get("price"))
                        .and_then(|p| p.get("perUnitLabel"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
        }
    }
    None
}

/// Check if product is purchasable from the nested offers structure
fn is_product_available(product: &Product) -> bool {
    let offers = &product.attributes.offers;
    if let Some(ean) = &product.attributes.ean {
        if let Some(ean_offers) = offers.get(ean) {
            if let Some(obj) = ean_offers.as_object() {
                if let Some((_, offer)) = obj.iter().next() {
                    return offer
                        .get("attributes")
                        .and_then(|a| a.get("availability"))
                        .and_then(|av| av.get("purchasable"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                }
            }
        }
    }
    false
}

fn matches_brand_filter(product: &Product, brand_filter: Option<&str>) -> bool {
    match brand_filter {
        None => true,
        Some(filter) => product
            .attributes
            .brand
            .as_ref()
            .map(|b| b.to_lowercase().contains(&filter.to_lowercase()))
            .unwrap_or(false),
    }
}

pub async fn search_products(
    query: &str,
    limit: u32,
    format: &str,
    brand_filter: Option<&str>,
    available_only: bool,
) -> Result<()> {
    let client = CarrefourClient::new()?;

    let response = client.search(query, limit).await?;
    let data = &response.data;

    // Apply filters to products
    let filtered_products: Vec<_> = data
        .products
        .iter()
        .filter(|p| !available_only || is_product_available(p))
        .filter(|p| matches_brand_filter(p, brand_filter))
        .take(limit as usize)
        .collect();

    // Get suggestion texts
    let suggestions: Vec<String> = data
        .filtered_suggestions
        .iter()
        .filter_map(|s| s.text.clone())
        .collect();

    if format == "json" {
        let json_output = serde_json::json!({
            "products": filtered_products,
            "suggestions": suggestions,
            "total": filtered_products.len()
        });
        println!("{}", serde_json::to_string_pretty(&json_output)?);
        return Ok(());
    }

    if filtered_products.is_empty() {
        println!("No products found for '{}'.", query);
        if !suggestions.is_empty() {
            println!(
                "{}",
                format!("Suggestions: {}", suggestions.join(", ")).dimmed()
            );
        }
        if brand_filter.is_some() || available_only {
            println!("{}", "(Try removing filters to see more results)".dimmed());
        }
        return Ok(());
    }

    let mut filter_info = String::new();
    if let Some(brand) = brand_filter {
        filter_info.push_str(&format!(" [brand: {}]", brand));
    }
    if available_only {
        filter_info.push_str(" [available only]");
    }

    println!(
        "Found {} products for '{}'{}:\n",
        filtered_products.len(),
        query,
        filter_info.dimmed()
    );

    for product in filtered_products {
        let attrs = &product.attributes;

        // Title
        println!("{}", attrs.title.bold());

        // Brand
        if let Some(brand) = &attrs.brand {
            println!("  Brand: {}", brand);
        }

        // Price
        if let Some(price) = get_product_price(product) {
            println!("  Price: {:.2}€", price);
        }

        // Unit price
        if let Some(per_unit) = get_per_unit_label(product) {
            println!("  Unit price: {}", per_unit);
        }

        // Availability
        let purchasable = is_product_available(product);
        let status = if purchasable {
            "In stock".green()
        } else {
            "Out of stock".red()
        };
        println!("  Status: {}", status);

        // Packaging
        if let Some(packaging) = &attrs.packaging {
            println!("  Format: {}", packaging);
        }

        // Origin
        if let Some(origin) = &attrs.origin {
            if !origin.is_empty() {
                println!("  Origin: {}", origin);
            }
        }

        // Nutriscore
        if let Some(nutriscore) = &attrs.nutriscore {
            if let Some(value) = &nutriscore.value {
                let colored_score = match value.as_str() {
                    "A" => value.green(),
                    "B" => value.bright_green(),
                    "C" => value.yellow(),
                    "D" => value.bright_red(),
                    "E" => value.red(),
                    _ => value.normal(),
                };
                println!("  Nutriscore: {}", colored_score);
            }
        }

        // EAN
        if let Some(ean) = &attrs.ean {
            println!("  EAN: {}", ean.dimmed());
        }

        println!();
    }

    Ok(())
}
