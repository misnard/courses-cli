use crate::client::CarrefourClient;
use crate::config::config_dir;
use crate::session::{self, has_sso, load_jar, merge, parse_cookies, save_jar, NEED_LOGIN, SSO_COOKIE};
use anyhow::Result;
use colored::Colorize;

pub fn set_session(session_id: &str) -> Result<()> {
    set_cookies(&format!("FRONTONE_SESSID={}", session_id))
}

/// Accepts a cookie header, a JSON map, or a JSON array export (the only
/// format carrying domains, so the only one that can import the SSO cookie).
pub fn set_cookies(cookies: &str) -> Result<()> {
    let mut jar = load_jar()?;
    merge(&mut jar, parse_cookies(cookies));
    save_jar(&jar)?;
    if !has_sso(&jar) {
        println!(
            "{}",
            format!("No {} cookie: the session will not renew itself.", SSO_COOKIE).yellow()
        );
    }
    Ok(())
}

pub fn logout() -> Result<()> {
    let _ = std::fs::remove_file(session::jar_path()?);
    let _ = std::fs::remove_dir_all(config_dir()?.join("carrefour-profile"));
    Ok(())
}

pub fn login() -> Result<()> {
    println!("A Chrome window opens: sign in (captcha + email code).");
    println!("{}", "Leave it open once signed in, it closes by itself.".dimmed());
    let cookies = session::login()?;
    if has_sso(&cookies) {
        println!(
            "{}",
            "Signed in! The session renews itself for up to 24 h (60 min idle).".green()
        );
    } else {
        println!(
            "{}",
            format!("Signed in, but no {} cookie came back: the session cannot renew itself.", SSO_COOKIE).yellow()
        );
    }
    Ok(())
}

pub fn refresh() -> Result<()> {
    session::Session::open()?.refresh()?;
    println!("{}", "Session refreshed.".green());
    Ok(())
}

pub fn show_status() -> Result<()> {
    let jar = load_jar()?;
    if jar.is_empty() {
        println!("{}", "Not authenticated.".yellow());
        println!("Run 'courses auth login' to sign in.");
        return Ok(());
    }

    println!("Validating session...\n");

    let client = CarrefourClient::new()?;
    match client.get_me() {
        Ok(profile) if profile.email.is_none() && profile.user_id.is_none() => {
            println!("{}", "Anonymous session (not signed in).".yellow());
            println!("Run 'courses auth login' to sign in.");
        }
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
            println!("\nPlease {}", NEED_LOGIN);
        }
    }

    if has_sso(&jar) {
        match client.refresh() {
            Ok(()) => println!("SSO session: {} (24 h max, 60 min idle; just renewed)", "alive".green()),
            Err(_) => println!("SSO session: {} — {}", "expired".red(), NEED_LOGIN),
        }
    } else {
        println!("SSO session: {} — the session cannot renew itself", "none".yellow());
    }

    Ok(())
}
