use anyhow::Result;
use console::style;

use crate::session::SessionStore;

pub fn run_list() -> Result<()> {
    let store = SessionStore::new()?;
    let sessions = store.list()?;

    if sessions.is_empty() {
        println!("{}", style("No sessions found.").dim());
        return Ok(());
    }

    // Header
    println!(
        "  {:<10} {:<16} {:<24} {:<20}",
        style("ID").bold(),
        style("Provider").bold(),
        style("Model").bold(),
        style("Last Active").bold(),
    );
    println!("  {}", style("─".repeat(70)).dim());

    for session in &sessions {
        let model = session.model.as_deref().unwrap_or("-");
        let last_active = session
            .last_active
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        println!(
            "  {:<10} {:<16} {:<24} {:<20}",
            style(session.short_id()).yellow(),
            style(session.provider.display_name()).cyan(),
            model,
            style(last_active).dim(),
        );
    }

    println!();
    println!(
        "  {} session(s)",
        style(sessions.len()).bold()
    );

    Ok(())
}

pub fn run_delete(id: &str) -> Result<()> {
    let store = SessionStore::new()?;

    match store.delete(id) {
        Ok(()) => {
            println!(
                "{} Deleted session {}",
                style("✓").green().bold(),
                style(id).yellow()
            );
        }
        Err(crate::error::SessionError::NotFound(msg)) => {
            println!(
                "{} {}",
                style("✗").red().bold(),
                msg
            );
        }
        Err(e) => {
            return Err(e.into());
        }
    }

    Ok(())
}
