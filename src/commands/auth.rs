use crate::client::CarrefourClient;
use crate::config::Config;
use crate::models::FlareSolverrResponse;
use anyhow::{Context, Result};
use colored::Colorize;

pub fn set_session(session_id: &str) -> Result<()> {
    let mut config = Config::load()?;
    config.session_id = Some(session_id.to_string());
    config.cookies = None; // Clear full cookies when setting session
    config.save()
}

pub fn set_cookies(cookies: &str) -> Result<()> {
    let mut config = Config::load()?;
    config.cookies = Some(cookies.to_string());
    config.session_id = None; // Clear session_id when using full cookies
    config.save()
}

pub fn logout() -> Result<()> {
    let mut config = Config::load()?;
    config.session_id = None;
    config.cookies = None;
    config.save()
}

pub async fn show_status() -> Result<()> {
    let config = Config::load()?;

    if config.session_id.is_none() && config.cookies.is_none() {
        println!("{}", "Not authenticated.".yellow());
        println!(
            "Run 'carrefour auth cookies --cookies <COOKIE_STRING>' to set your cookies.\n\
             Get cookies from browser DevTools: Application > Cookies > www.carrefour.fr"
        );
        return Ok(());
    }

    println!("Validating session...\n");

    let client = CarrefourClient::new()?;
    match client.get_me().await {
        Ok(profile) => {
            println!("{}", "Authenticated".green().bold());

            if let (Some(first), Some(last)) = (&profile.first_name, &profile.last_name) {
                println!("Name: {} {}", first, last);
            }

            if let Some(email) = &profile.email {
                let masked = if email.len() > 8 {
                    let at_pos = email.find('@').unwrap_or(email.len() / 2);
                    format!(
                        "{}...{}",
                        &email[..3.min(at_pos)],
                        &email[at_pos.saturating_sub(0)..]
                    )
                } else {
                    "***".to_string()
                };
                println!("Email: {}", masked);
            }

            if let Some(card) = &profile.loyalty_card {
                let masked_card = if card.number.len() > 8 {
                    format!(
                        "{}...{}",
                        &card.number[..4],
                        &card.number[card.number.len() - 4..]
                    )
                } else {
                    card.number.clone()
                };
                println!("Loyalty card: {} ({})", masked_card, card.card_type);
            }

            if let Some(slot) = &profile.slot {
                if let (Some(title), Some(hour)) = (&slot.title, &slot.hour) {
                    println!("Current slot: {} at {}", title, hour);
                }
            }
        }
        Err(e) => {
            println!("{}", "Session invalid or expired.".red());
            println!("Error: {}", e);
            println!(
                "\nPlease get a new FRONTONE_SESSID cookie from your browser\n\
                 and run 'carrefour auth set --session <SESSID>'"
            );
        }
    }

    Ok(())
}

const DEFAULT_FLARESOLVERR_URL: &str = "http://localhost:8191/v1";

pub async fn fetch_cookies_flaresolverr(flaresolverr_url: Option<&str>) -> Result<()> {
    let url = flaresolverr_url.unwrap_or(DEFAULT_FLARESOLVERR_URL);

    println!("Connecting to FlareSolverr at {}...", url.cyan());
    println!("This may take 30-60 seconds while Cloudflare challenges are solved.\n");

    // Create a simple HTTP client for FlareSolverr
    let client = rquest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .context("Failed to create HTTP client")?;

    // Step 1: Create a session to maintain browser state
    println!("Step 1/3: Creating browser session...");
    let create_session = serde_json::json!({
        "cmd": "sessions.create"
    });

    let response = client
        .post(url)
        .json(&create_session)
        .send()
        .await
        .context("Failed to connect to FlareSolverr. Is it running?\n\nStart it with: docker run -d -p 8191:8191 ghcr.io/flaresolverr/flaresolverr:latest")?;

    let session_response: serde_json::Value = response.json().await?;
    let session_id = session_response["session"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to create FlareSolverr session"))?
        .to_string();

    println!("  Session created: {}", session_id.dimmed());

    // Step 2: Visit homepage to get initial cookies and solve challenge
    println!("Step 2/3: Solving homepage challenge...");
    let homepage_request = serde_json::json!({
        "cmd": "request.get",
        "url": "https://www.carrefour.fr/",
        "session": session_id,
        "maxTimeout": 60000
    });

    let response = client.post(url).json(&homepage_request).send().await?;
    let homepage_response: FlareSolverrResponse = response.json().await?;

    if homepage_response.status != "ok" {
        // Cleanup session on failure
        let _ = client
            .post(url)
            .json(&serde_json::json!({"cmd": "sessions.destroy", "session": session_id}))
            .send()
            .await;
        anyhow::bail!(
            "FlareSolverr failed on homepage: {}",
            homepage_response.message
        );
    }

    println!("  Homepage loaded successfully");

    // Step 3: Visit the API endpoint to get cookies valid for API
    println!("Step 3/3: Solving API challenge...");
    let api_request = serde_json::json!({
        "cmd": "request.get",
        "url": "https://www.carrefour.fr/api/me",
        "session": session_id,
        "maxTimeout": 60000
    });

    let response = client.post(url).json(&api_request).send().await?;
    let api_response: FlareSolverrResponse = response.json().await?;

    // Cleanup session
    let _ = client
        .post(url)
        .json(&serde_json::json!({"cmd": "sessions.destroy", "session": session_id}))
        .send()
        .await;

    if api_response.status != "ok" {
        anyhow::bail!("FlareSolverr failed on API: {}", api_response.message);
    }

    let solution = api_response
        .solution
        .ok_or_else(|| anyhow::anyhow!("FlareSolverr returned no solution"))?;

    if solution.cookies.is_empty() {
        anyhow::bail!("FlareSolverr returned no cookies");
    }

    // Convert cookies to cookie string format
    let cookie_string: String = solution
        .cookies
        .iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ");

    // Check if we got the essential cookies
    let has_session = solution.cookies.iter().any(|c| c.name == "FRONTONE_SESSID");
    let has_cf_bm = solution.cookies.iter().any(|c| c.name == "__cf_bm");
    let has_cf_clearance = solution.cookies.iter().any(|c| c.name == "cf_clearance");

    println!("\n{}", "Cookies obtained successfully!".green());
    println!("  Cookies received: {}", solution.cookies.len());
    println!(
        "  FRONTONE_SESSID: {}",
        if has_session {
            "✓".green()
        } else {
            "✗ (not logged in)".yellow()
        }
    );
    println!(
        "  __cf_bm: {}",
        if has_cf_bm {
            "✓".green()
        } else {
            "✗".red()
        }
    );
    println!(
        "  cf_clearance: {}",
        if has_cf_clearance {
            "✓".green()
        } else {
            "(not present)".dimmed()
        }
    );
    println!("  User-Agent: {}", solution.user_agent.dimmed());

    // Save cookies and user agent to config
    let mut config = Config::load()?;
    config.cookies = Some(cookie_string);
    config.user_agent = Some(solution.user_agent);
    config.session_id = None;
    config.save()?;

    println!("\n{}", "Cookies saved to config!".green().bold());

    if !has_session {
        println!(
            "\n{}",
            "Note: No session cookie found. You may need to log in at carrefour.fr first,".yellow()
        );
        println!(
            "{}",
            "then run this command again to capture authenticated cookies.".yellow()
        );
    }

    Ok(())
}
