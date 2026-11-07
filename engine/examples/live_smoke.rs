//! Explicit public-source checks only. Does not inspect installed software.
use releasescout_engine::{
    catalog,
    http::{CacheOptions, HttpClient},
    *,
};
use serde_json::json;

fn main() -> Result<()> {
    let engine = Engine {
        catalog: catalog::bundled()?,
        fetcher: HttpClient::new(CacheOptions {
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            ..Default::default()
        })?,
    };
    let requested: Vec<String> = std::env::args().skip(1).collect();
    let defaults = [
        "atlas-cli",
        "agent-browser",
        "gh",
        "pnpm",
        "uv",
        "ripgrep",
        "deno",
        "fzf",
        "jq",
        "k3d",
        "k9s",
        "opencode",
        "superfile",
        "upm-cli",
        "claude-code",
        "cloudflared",
        "mise",
        "nginx",
        "pdfgear",
        "slack",
        "claude-desktop",
        "notion",
        "figma",
        "supabase",
    ];
    let ids: Vec<&str> = if requested.is_empty() {
        defaults.to_vec()
    } else {
        requested.iter().map(String::as_str).collect()
    };
    std::fs::create_dir_all("artifacts/live")?;
    let mut summary = vec![];
    let mut failures = 0;
    for id in ids {
        let component = if id == "supabase" {
            Some("hosted")
        } else {
            None
        };
        match engine.notes(id, component, Selection::Inspect, false) {
            Ok(inspect) => {
                std::fs::write(
                    format!("artifacts/live/{id}-inspect.json"),
                    serde_json::to_vec_pretty(&inspect)?,
                )?;
                let selection = match inspect.basis {
                    Basis::Version => {
                        let notes: Vec<_> = inspect.notes.iter().filter(|n| !n.preview).collect();
                        if notes.len() >= 2 {
                            Some(Selection::Versions {
                                from: notes[1].version.clone().unwrap(),
                                to: notes[0].version.clone().unwrap(),
                                channel: Channel::Stable,
                            })
                        } else {
                            None
                        }
                    }
                    Basis::Date => inspect
                        .notes
                        .iter()
                        .filter_map(|n| n.date)
                        .min()
                        .filter(|date| *date < chrono::Utc::now().date_naive())
                        .map(|since| Selection::Dates {
                            since,
                            until: chrono::Utc::now().date_naive(),
                        }),
                };
                let mut range = None;
                if let Some(selection) = selection {
                    let report = engine.notes(id, component, selection, false)?;
                    if !report.notes.iter().any(|n| n.authored_notes) {
                        failures += 1;
                    }
                    std::fs::write(
                        format!("artifacts/live/{id}-range.json"),
                        serde_json::to_vec_pretty(&report)?,
                    )?;
                    range = Some(
                        json!({"status":report.status,"complete":report.complete,"notes":report.notes.len(),"authored":report.notes.iter().filter(|n|n.authored_notes).count()}),
                    );
                }
                if range.is_none() {
                    failures += 1;
                }
                let entry = json!({"id":id,"status":inspect.status,"entries":inspect.notes.len(),"warnings":inspect.warnings,"attempts":inspect.attempts,"range":range});
                println!(
                    "{}: {} ({} entries)",
                    id,
                    inspect.status,
                    inspect.notes.len()
                );
                summary.push(entry);
            }
            Err(error) => {
                failures += 1;
                eprintln!("{id}: {error}");
                summary.push(json!({"id":id,"error":error.to_string()}));
            }
        }
        // Persist progress even when a later source is unavailable.
        std::fs::write(
            "artifacts/live/summary.json",
            serde_json::to_vec_pretty(&summary)?,
        )?;
    }
    if failures > 0 {
        return Err(Error::Source(format!(
            "{failures} live checks did not produce an authored-note range; see artifacts/live/summary.json"
        )));
    }
    Ok(())
}
