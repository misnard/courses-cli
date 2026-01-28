mod client;
mod commands;
mod config;
mod error;
mod models;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use commands::{auth, cart, search, slots};
use std::io;

#[derive(Parser)]
#[command(name = "carrefour")]
#[command(about = "CLI tool to search and order from Carrefour", long_about = None)]
struct Cli {
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
    },
    /// Manage shopping cart
    Cart {
        #[command(subcommand)]
        action: CartCommands,
    },
    /// Manage delivery/pickup time slots
    Slots {
        #[command(subcommand)]
        action: SlotsCommands,
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
    /// Set session cookie (legacy, use 'cookies' for full support)
    Set {
        /// FRONTONE_SESSID cookie value from browser
        #[arg(short, long)]
        session: String,
    },
    /// Set full cookie string (for Cloudflare bypass)
    Cookies {
        /// Full cookie string from browser (copy from DevTools)
        #[arg(short, long)]
        cookies: String,
    },
    /// Fetch cookies automatically via FlareSolverr (requires FlareSolverr running)
    Flaresolverr {
        /// FlareSolverr URL (default: http://localhost:8191/v1)
        #[arg(short, long)]
        url: Option<String>,
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
        /// Product EAN (barcode)
        ean: String,
        /// Quantity
        #[arg(short, long, default_value = "1")]
        quantity: u32,
    },
    /// Remove product from cart
    Remove {
        /// Product EAN (barcode)
        ean: String,
    },
    /// Update product quantity in cart
    Update {
        /// Product EAN (barcode)
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Auth { action } => match action {
            AuthCommands::Set { session } => {
                auth::set_session(&session)?;
                println!("Session saved successfully!");
            }
            AuthCommands::Cookies { cookies } => {
                auth::set_cookies(&cookies)?;
                println!("Cookies saved successfully!");
            }
            AuthCommands::Flaresolverr { url } => {
                auth::fetch_cookies_flaresolverr(url.as_deref()).await?;
            }
            AuthCommands::Status => {
                auth::show_status().await?;
            }
            AuthCommands::Logout => {
                auth::logout()?;
                println!("Session cleared.");
            }
        },
        Commands::Search {
            query,
            limit,
            format,
            brand,
            available,
        } => {
            search::search_products(&query, limit, &format, brand.as_deref(), available).await?;
        }
        Commands::Cart { action } => match action {
            CartCommands::Show { format } => {
                cart::show_cart(&format).await?;
            }
            CartCommands::Add { ean, quantity } => {
                cart::add_to_cart(&ean, quantity).await?;
            }
            CartCommands::Remove { ean } => {
                cart::remove_from_cart(&ean).await?;
            }
            CartCommands::Update { ean, quantity } => {
                cart::update_quantity(&ean, quantity).await?;
            }
            CartCommands::Clear => {
                cart::clear_cart().await?;
            }
        },
        Commands::Slots { action } => match action {
            SlotsCommands::Show { format } => {
                slots::show_slots(&format).await?;
            }
            SlotsCommands::Select { slot_ref } => {
                slots::select_slot(&slot_ref).await?;
            }
        },
        Commands::Config => {
            config::show_config()?;
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            generate(shell, &mut cmd, name, &mut io::stdout());
        }
    }

    Ok(())
}
