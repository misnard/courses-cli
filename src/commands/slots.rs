use crate::client::CarrefourClient;
use anyhow::Result;
use colored::Colorize;

pub fn show_slots(format: &str) -> Result<()> {
    let client = CarrefourClient::new()?;

    let response = client.get_timeslots(None)?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }

    if response.timeslots.is_empty() {
        println!("No available time slots.");
        return Ok(());
    }

    println!("{}", "Available Time Slots".bold().underline());
    println!();

    for day in &response.timeslots {
        let day_header = format!(
            "{} {}",
            day.day.as_deref().unwrap_or(""),
            day.date.as_deref().unwrap_or("")
        );
        println!("{}", format!("── {} ──", day_header.trim()).cyan());

        if day.cells.is_empty() {
            println!("  No slots available");
            continue;
        }

        for slot in &day.cells {
            let available = slot.available.unwrap_or(false);
            let selected = slot.selected.unwrap_or(false);

            let status_icon = if selected {
                "●".green()
            } else if available {
                "○".white()
            } else {
                "✗".red()
            };

            let time_str = match (&slot.hour_start, &slot.hour_end) {
                (Some(start), Some(end)) => format!("{}-{}", start, end),
                (Some(start), None) => start.clone(),
                _ => "--:--".to_string(),
            };

            let status_text = if selected {
                " (selected)".green().to_string()
            } else if !available {
                " (full)".dimmed().to_string()
            } else {
                String::new()
            };

            let fees_str = slot
                .fees
                .map(|f| format!(" - {:.2}€", f))
                .unwrap_or_default();

            let fill_str = slot
                .fill_rate
                .map(|r| format!(" [{}%]", r).dimmed().to_string())
                .unwrap_or_default();

            println!(
                "  {} {}{}{}{}",
                status_icon,
                time_str.bold(),
                fees_str,
                fill_str,
                status_text
            );

            // Show slot ref for booking (dimmed)
            if available && !selected {
                println!("      ref: {}", slot.slot_ref.dimmed());
            }
        }
        println!();
    }

    println!(
        "{}",
        "Use 'courses slots select <REF>' to select a slot.".dimmed()
    );

    Ok(())
}

pub fn select_slot(slot_ref: &str) -> Result<()> {
    let client = CarrefourClient::new()?;

    println!("Selecting slot {}...", slot_ref);

    client.select_slot(slot_ref)?;

    println!("{}", "Slot selected!".green());

    Ok(())
}
