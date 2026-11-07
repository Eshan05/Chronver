use releasescout_engine::watchlist::{self, Entry};
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: import_watchlist SOURCE.json DESTINATION.json".into());
    }
    let incoming: Vec<Entry> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let path = PathBuf::from(&args[1]);
    let mut entries = watchlist::load(&path)?;
    let before = entries.len();
    for entry in incoming {
        entry.validate()?;
        if !entries.iter().any(|e| e.id() == entry.id()) {
            entries.push(entry);
        }
    }
    watchlist::save(&path, &entries)?;
    println!(
        "{} tracked packages ({} added)",
        entries.len(),
        entries.len() - before
    );
    Ok(())
}
