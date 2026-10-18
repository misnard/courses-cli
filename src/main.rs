mod browser;
mod client;
mod commands;
mod config;
mod error;
mod models;
mod openfoodfacts;
mod providers;
mod session;
mod spec;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use commands::{api, auth, shop, slots};
use providers::Provider;
use std::io;

#[derive(Parser)]
#[command(name = "courses")]
#[command(about = "Do your grocery shopping at French drives from the terminal", long_about = None)]
struct Cli {
    /// Store chain to shop at
    #[arg(long, global = true, value_enum, env = "COURSES_PROVIDER", default_value = "carrefour")]
    provider: Provider,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage authentication
    Auth {
        #[command(subcommand)]
        action: AuthCommands,
    },
    /// Search for products
    Search {
        /// Search query
        query: String,
        /// Maximum number of results
        #[arg(short, long, default_value = "10")]
        limit: u32,
        /// Output format (table or json)
        #[arg(short, long, default_value = "table")]
        format: String,
        /// Filter by brand name
        #[arg(short, long)]
        brand: Option<String>,
        /// Only show available products
        #[arg(short, long)]
        available: bool,
        /// Fill missing Nutri-Scores from Open Food Facts (by EAN)
        #[arg(long)]
        nutriscore: bool,
    },
    /// Manage shopping cart
    Cart {
        #[command(subcommand)]
        action: CartCommands,
    },
    /// Manage delivery/pickup time slots (Carrefour)
    Slots {
        #[command(subcommand)]
        action: SlotsCommands,
    },
    /// Call any of the 43 carrefour.fr endpoints (no name: list them)
    Api {
        /// Endpoint name, e.g. get_orders
        name: Option<String>,
        /// Parameters as param=value (arrays: JSON or comma list)
        params: Vec<String>,
        /// Show the endpoint's description and parameters
        #[arg(short, long)]
        describe: bool,
        /// Confirm endpoints that charge a real payment
        #[arg(long)]
        yes: bool,
    },
    /// Find a drive near a postal code or city and pick it
    Store {
        /// Postal code or city (none: show the picked store)
        query: Option<String>,
        /// Store id from the list to shop at
        #[arg(long)]
        pick: Option<String>,
    },
    /// Show current configuration
    Config,
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Sign in through a Chrome window (captcha + email code)
    Login,
    /// Renew the storefront session from the SSO cookie (Carrefour)
    Refresh,
    /// Local web page to sign in and export/import cookies (Carrefour, 127.0.0.1 only)
    Serve {
        #[arg(short, long, default_value = "8765")]
        port: u16,
    },
    /// Set session cookie (Carrefour, legacy: use 'cookies')
    Set {
        /// FRONTONE_SESSID cookie value from browser
        #[arg(short, long)]
        session: String,
    },
    /// Import cookies: header string, JSON map, or JSON array export (Carrefour)
    Cookies {
        /// Cookies from browser DevTools (a JSON array export keeps the SSO cookie)
        #[arg(short, long)]
        cookies: String,
    },
    /// Show authentication status and user info
    Status,
    /// Clear saved session
    Logout,
}

#[derive(Subcommand)]
enum CartCommands {
    /// Show current cart
    Show {
        /// Output format (table or json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Add product to cart
    Add {
        /// Product ID from search (the EAN at Carrefour)
        ean: String,
        /// Quantity
        #[arg(short, long, default_value = "1")]
        quantity: u32,
    },
    /// Remove product from cart
    Remove {
        /// Product ID from search (the EAN at Carrefour)
        ean: String,
    },
    /// Update product quantity in cart
    Update {
        /// Product ID from search (the EAN at Carrefour)
        ean: String,
        /// New quantity
        #[arg(short, long)]
        quantity: u32,
    },
    /// Clear all items from cart
    Clear,
}

#[derive(Subcommand)]
enum SlotsCommands {
    /// Show available time slots
    Show {
        /// Output format (table or json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Select a time slot
    Select {
        /// Slot reference ID
        slot_ref: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let p = cli.provider;
    let carrefour = p == Provider::Carrefour;

    match cli.command {
        Commands::Auth { action } if !carrefour => match action {
            AuthCommands::Login => shop::login(p)?,
            AuthCommands::Status => shop::status(p)?,
            AuthCommands::Logout => {
                providers::logout(p)?;
                println!("Session cleared.");
            }
            _ => bail!("Only login, status and logout exist for {}.", p.name()),
        },
        Commands::Slots { .. } | Commands::Api { .. } if !carrefour => {
            bail!("Only available for Carrefour: book the slot on the {} site.", p.name())
        }
        Commands::Auth { action } => match action {
            AuthCommands::Login => auth::login()?,
            AuthCommands::Refresh => auth::refresh()?,
            AuthCommands::Serve { port } => commands::serve::serve(port)?,
            AuthCommands::Set { session } => {
                auth::set_session(&session)?;
                println!("Session saved successfully!");
            }
            AuthCommands::Cookies { cookies } => {
                auth::set_cookies(&cookies)?;
                println!("Cookies saved successfully!");
            }
            AuthCommands::Status => auth::show_status()?,
            AuthCommands::Logout => {
                auth::logout()?;
                println!("Session cleared.");
            }
        },
        Commands::Search { query, limit, format, brand, available, nutriscore } => {
            shop::search(p, &query, limit, &format, brand.as_deref(), available, nutriscore)?
        }
        Commands::Cart { action } => match action {
            CartCommands::Show { format } => shop::show_cart(p, &format)?,
            CartCommands::Add { ean, quantity } | CartCommands::Update { ean, quantity } => {
                shop::set_quantity(p, &ean, quantity)?
            }
            CartCommands::Remove { ean } => shop::set_quantity(p, &ean, 0)?,
            CartCommands::Clear => shop::clear_cart(p)?,
        },
        Commands::Slots { action } => match action {
            SlotsCommands::Show { format } => slots::show_slots(&format)?,
            SlotsCommands::Select { slot_ref } => slots::select_slot(&slot_ref)?,
        },
        Commands::Store { query, pick } => shop::store(p, query.as_deref(), pick.as_deref())?,
        Commands::Api { name, params, describe, yes } => match name {
            None => api::list(),
            Some(name) if describe => api::describe(&name)?,
            Some(name) => api::call(&name, &params, yes)?,
        },
        Commands::Config => config::show_config()?,
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            generate(shell, &mut cmd, name, &mut io::stdout());
        }
    }

    Ok(())
}
