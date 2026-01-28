use crate::client::CarrefourClient;
use anyhow::Result;
use colored::Colorize;

pub async fn show_cart(format: &str) -> Result<()> {
    let client = CarrefourClient::new()?;

    let response = client.get_cart().await?;
    let cart = &response.cart;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }

    if cart.items.is_empty() {
        println!("Your cart is empty.");
        return Ok(());
    }

    println!("{}", "Shopping Cart".bold().underline());
    println!("Cart ID: {}\n", cart.cart_id.dimmed());

    for category in &cart.items {
        println!("{}", format!("── {} ──", category.category).cyan());

        for item in &category.products {
            let product = &item.product;
            let attrs = &product.attributes;

            let availability = if item.available {
                "✓".green()
            } else {
                "✗".red()
            };

            println!(
                "  {} {} x{} - {:.2}{}",
                availability,
                attrs.title.bold(),
                item.counter,
                item.total_item_price,
                cart.currency
            );

            if let Some(brand) = &attrs.brand {
                println!("      Brand: {}", brand);
            }

            if let Some(packaging) = &attrs.packaging {
                println!("      {}", packaging.dimmed());
            }

            if let Some(ean) = &attrs.ean {
                println!("      EAN: {}", ean.dimmed());
            }
        }
        println!();
    }

    println!("{}", "─".repeat(40));
    println!("Products: {} items", cart.nb_products);
    println!("Subtotal: {:.2}{}", cart.total_price, cart.currency);

    if cart.total_fees > 0.0 {
        println!("Fees (bags): {:.2}{}", cart.total_fees, cart.currency);
    }

    println!(
        "{}: {:.2}{}",
        "Total".bold(),
        cart.total_amount,
        cart.currency
    );

    if let Some(slot) = &cart.slot {
        if let Some(hour) = &slot.hour {
            println!("\nPickup slot: {}", hour.green());
        }
    }

    Ok(())
}

pub async fn add_to_cart(ean: &str, quantity: u32) -> Result<()> {
    let client = CarrefourClient::new()?;

    println!("Adding {} (x{}) to cart...", ean, quantity);

    client.add_to_cart(ean, quantity).await?;

    println!("{}", "Item added to cart!".green());

    Ok(())
}

pub async fn remove_from_cart(ean: &str) -> Result<()> {
    let client = CarrefourClient::new()?;

    println!("Removing {} from cart...", ean);

    client.remove_from_cart(ean).await?;

    println!("{}", "Item removed from cart!".green());

    Ok(())
}

pub async fn update_quantity(ean: &str, quantity: u32) -> Result<()> {
    let client = CarrefourClient::new()?;

    println!("Updating {} quantity to {}...", ean, quantity);

    client.update_cart_quantity(ean, quantity).await?;

    println!("{}", "Cart updated!".green());

    Ok(())
}

pub async fn clear_cart() -> Result<()> {
    let client = CarrefourClient::new()?;

    println!("Clearing cart...");

    // First get the cart to find all product EANs
    let response = client.get_cart().await?;
    let cart = &response.cart;

    if cart.items.is_empty() {
        println!("Cart is already empty.");
        return Ok(());
    }

    // Remove each product
    let mut removed = 0;
    for category in &cart.items {
        for item in &category.products {
            if let Some(ean) = &item.product.attributes.ean {
                if let Err(e) = client.remove_from_cart(ean).await {
                    eprintln!("Warning: failed to remove {}: {}", ean, e);
                } else {
                    removed += 1;
                }
            }
        }
    }

    println!(
        "{}",
        format!("Removed {} items from cart.", removed).green()
    );

    Ok(())
}
