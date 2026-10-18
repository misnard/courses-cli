mod browser;
mod client;
mod commands;
mod config;
mod error;
mod leclerc;
mod models;
mod session;
mod spec;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use commands::{api, auth, cart, search, slots};
use std::io;

#[derive(Parser)]
#[command(name = "carrefour")]
#[command(about = "CLI tool to search and order from Carrefour or E.Leclerc Drive", long_about = None)]
struct Cli {
    /// Store chain to shop at
    #[arg(long, global = true, value_enum, env = "CARREFOUR_PROVIDER", default_value = "carrefour")]
    provider: Provider,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Provider {
    Carrefour,
    Leclerc,
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
    /// Find a drive near a postal code or city and pick it (Leclerc)
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
    /// Renew the storefront session from the SSO cookie
    Refresh,
    /// Local web page to sign in and export/import cookies (127.0.0.1 only)
    Serve {
        #[arg(short, long, default_value = "8765")]
        port: u16,
    },
    /// Set session cookie (legacy, use 'cookies' for full support)
    Set {
        /// FRONTONE_SESSID cookie value from browser
        #[arg(short, long)]
        session: String,
    },
    /// Import cookies: header string, JSON map, or JSON array export
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
        /// Product EAN (barcode); Leclerc: product ID from search
        ean: String,
        /// Quantity
        #[arg(short, long, default_value = "1")]
        quantity: u32,
    },
    /// Remove product from cart
    Remove {
        /// Product EAN (barcode); Leclerc: product ID from search
        ean: String,
    },
    /// Update product quantity in cart
    Update {
        /// Product EAN (barcode); Leclerc: product ID from search
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
    let leclerc = cli.provider == Provider::Leclerc;

    match cli.command {
        Commands::Auth { action } if leclerc => match action {
            AuthCommands::Login => leclerc::login()?,
            AuthCommands::Status => leclerc::status()?,
            AuthCommands::Logout => {
                leclerc::logout()?;
                println!("Session cleared.");
            }
            _ => bail!("Only login, status and logout exist for Leclerc."),
        },
        Commands::Search { query, limit, format, brand, available } if leclerc => {
            leclerc::search(&query, limit, &format, brand.as_deref(), available)?
        }
        Commands::Cart { action } if leclerc => match action {
            CartCommands::Show { format } => leclerc::show_cart(&format)?,
            CartCommands::Add { ean, quantity } | CartCommands::Update { ean, quantity } => {
                leclerc::set_quantity(&ean, quantity)?
            }
            CartCommands::Remove { ean } => leclerc::set_quantity(&ean, 0)?,
            CartCommands::Clear => leclerc::clear_cart()?,
        },
        Commands::Slots { .. } | Commands::Api { .. } if leclerc => {
            bail!("Not available for Leclerc yet: book the slot on leclercdrive.fr.")
        }
        Commands::Store { query, pick } if leclerc => leclerc::store_cmd(query.as_deref(), pick.as_deref())?,
        Commands::Store { .. } => bail!("Pick your Carrefour store on carrefour.fr, then run 'carrefour auth refresh'."),
        Commands::Auth { action } => match action {
            AuthCommands::Login => auth::login()?,
            AuthCommands::Refresh => auth::refresh()?,
            AuthCommands::Serve { port } => commands::serve::serve(port).await?,
            AuthCommands::Set { session } => {
                auth::set_session(&session)?;
                println!("Session saved successfully!");
            }
            AuthCommands::Cookies { cookies } => {
                auth::set_cookies(&cookies)?;
                println!("Cookies saved successfully!");
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
        Commands::Api {
            name,
            params,
            describe,
            yes,
        } => match name {
            None => api::list(),
            Some(name) if describe => api::describe(&name)?,
            Some(name) => api::call(&name, &params, yes)?,
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
