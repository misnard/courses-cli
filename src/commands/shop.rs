//! The commands every provider shares: they print what its `Shop` returns.

use crate::providers::{self, Product, Provider, Store};
use anyhow::{bail, Context, Result};
use colored::Colorize;
use serde_json::json;

fn money(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.2}€")).unwrap_or_else(|| "?".into())
}

fn nutriscore(n: &str) -> colored::ColoredString {
    match n {
        "A" => n.green(),
        "B" => n.bright_green(),
        "C" => n.yellow(),
        "D" => n.bright_red(),
        "E" => n.red(),
        _ => n.normal(),
    }
}

pub fn search(p: Provider, query: &str, limit: u32, format: &str, brand: Option<&str>, available_only: bool, fill_nutriscore: bool) -> Result<()> {
    let brand = brand.map(str::to_lowercase);
    let mut products: Vec<Product> = providers::open(p, true)?
        .search(query, limit)?
        .into_iter()
        .filter(|x| !available_only || x.available)
        // Without a brand field, the brand is part of the name.
        .filter(|x| {
            brand.as_ref().is_none_or(|b| x.brand.as_ref().unwrap_or(&x.name).to_lowercase().contains(b))
        })
        .take(limit as usize)
        .collect();
    // Optional extra: a failure costs the scores, not the results.
    if fill_nutriscore {
        if let Err(e) = crate::openfoodfacts::fill_nutriscores(&mut products) {
            eprintln!("Warning: no Nutri-Score from Open Food Facts: {e}");
        }
    }

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&json!({"products": products, "total": products.len()}))?);
        return Ok(());
    }
    if products.is_empty() {
        println!("No products found for '{}'.", query);
        if brand.is_some() || available_only {
            println!("{}", "(Try removing filters to see more results)".dimmed());
        }
        return Ok(());
    }
    println!("Found {} products for '{}':\n", products.len(), query);
    for x in &products {
        println!("{}", x.name.bold());
        if let Some(b) = &x.brand {
            println!("  Brand: {}", b);
        }
        println!("  Price: {}", money(x.price));
        if let Some(u) = &x.unit_price {
            println!("  Unit price: {}", u);
        }
        println!("  Status: {}", if x.available { "In stock".green() } else { "Out of stock".red() });
        if let Some(f) = &x.packaging {
            println!("  Format: {}", f);
        }
        if let Some(n) = &x.nutriscore {
            println!("  Nutriscore: {}", nutriscore(n));
        }
        if x.in_cart > 0 {
            println!("  In cart: {}", x.in_cart);
        }
        println!("  ID: {}\n", x.id.dimmed());
    }
    Ok(())
}

pub fn show_cart(p: Provider, format: &str) -> Result<()> {
    let cart = providers::open(p, true)?.cart()?;
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&cart)?);
        return Ok(());
    }
    if cart.lines.is_empty() {
        println!("Your cart is empty.");
        return Ok(());
    }
    println!("{}\n", "Shopping Cart".bold().underline());
    for l in &cart.lines {
        let ok = if l.product.available { "✓".green() } else { "✗".red() };
        println!("  {} {} x{} - {}", ok, l.product.name.bold(), l.quantity, money(l.total));
        println!("      ID: {}", l.product.id.dimmed());
    }
    println!("\n{}", "─".repeat(40));
    println!("Products: {} items", cart.lines.iter().map(|l| l.quantity).sum::<u32>());
    println!("{}: {}", "Total".bold(), money(cart.total));
    if let Some(n) = &cart.note {
        println!("\n{}", n.green());
    }
    Ok(())
}

pub fn set_quantity(p: Provider, id: &str, quantity: u32) -> Result<()> {
    let held = providers::open(p, true)?.set_quantity(id, quantity)?;
    if held != quantity {
        bail!("Product {id} is at quantity {held}, not {quantity}: unknown ID or not enough stock.");
    }
    println!("{}", format!("Product {id}: quantity {quantity}.").green());
    Ok(())
}

pub fn clear_cart(p: Provider) -> Result<()> {
    let mut shop = providers::open(p, true)?;
    let cart = shop.cart()?;
    if cart.lines.is_empty() {
        println!("Cart is already empty.");
        return Ok(());
    }
    let mut removed = 0;
    for (i, l) in cart.lines.iter().enumerate() {
        if i > 0 {
            // Bot protections (DataDome) flag bursts of cart writes.
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        match shop.set_quantity(&l.product.id, 0) {
            Ok(_) => removed += 1,
            Err(e) => eprintln!("Warning: failed to remove {}: {}", l.product.id, e),
        }
    }
    println!("{}", format!("Removed {} items from cart.", removed).green());
    Ok(())
}

/// No query: show the picked store. Otherwise list stores near `query`, or
/// remember the one whose id is `pick`.
pub fn store(p: Provider, query: Option<&str>, pick: Option<&str>) -> Result<()> {
    let Some(query) = query else {
        let s = providers::store(p)?;
        println!("{} ({}) {}", s.name.bold(), s.id, s.info.dimmed());
        return Ok(());
    };
    let stores = providers::open(p, true)?.find_stores(query)?;
    if let Some(pick) = pick {
        let s: Store = stores
            .into_iter()
            .find(|s| s.id == pick)
            .with_context(|| format!("No store {pick} near '{query}'."))?;
        println!("{}", format!("Shopping at {} ({}).", s.name, s.id).green());
        return providers::save_store(p, s);
    }
    if stores.is_empty() {
        println!("No store found near '{}'.", query);
        return Ok(());
    }
    for s in &stores {
        println!("{:<10} {:<40} {}", s.id, s.name.bold(), s.info.dimmed());
    }
    println!("\n{}", format!("Pick one: {} store {query} --pick <id>", p.cmd()).dimmed());
    Ok(())
}

pub fn login(p: Provider) -> Result<()> {
    let mut shop = providers::open(p, false)?;
    shop.login()?;
    if shop.signed_in()? {
        println!("{}", "Signed in!".green());
    } else {
        println!("{}", "Not signed in: the CLI keeps an anonymous session.".yellow());
    }
    Ok(())
}

pub fn checkout(p: Provider) -> Result<()> {
    providers::open(p, false)?.checkout()
}

pub fn status(p: Provider) -> Result<()> {
    if let Ok(s) = providers::store(p) {
        println!("Store: {} ({})", s.name, s.id);
    }
    if providers::open(p, true)?.signed_in()? {
        println!("{}", "Authenticated".green().bold());
    } else {
        println!("{}", "Anonymous session (not signed in).".yellow());
    }
    Ok(())
}
