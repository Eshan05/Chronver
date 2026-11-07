use crate::background_watch::Event;
use releasescout_engine::{Error, Result};

pub const WORKER_ARG: &str = "--background-watch";
const URI_PREFIX: &str = "chronver-watch:";

pub fn activation(arg: &str) -> Option<&str> {
    let target = arg.strip_prefix(URI_PREFIX)?;
    (target == "all" || target.len() == 64 && target.bytes().all(|b| b.is_ascii_hexdigit()))
        .then_some(target)
}

#[cfg(windows)]
fn powershell(script: &str, env: &[(&str, &str)]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let shell = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()),
    )
    .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let script = format!(
        "try {{\n{script}\n}} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}"
    );
    let output = std::process::Command::new(shell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ])
        .envs(env.iter().copied())
        .creation_flags(0x08000000)
        .output()?;
    if !output.status.success() {
        let detail: String = String::from_utf8_lossy(&output.stderr)
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("Windows command failed")
            .chars()
            .take(230)
            .collect();
        return Err(Error::Source(detail));
    }
    Ok(())
}

#[cfg(windows)]
pub fn schedule(enabled: bool) -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe = exe
        .to_str()
        .ok_or_else(|| Error::Config("Chronver path is not valid text".into()))?;
    powershell(
        include_str!("background_schedule.ps1"),
        &[
            ("CHRONVER_WATCH_EXE", exe),
            ("CHRONVER_WATCH_ACTION", WORKER_ARG),
            ("CHRONVER_WATCH_ENABLE", if enabled { "1" } else { "0" }),
        ],
    )
}

#[cfg(not(windows))]
pub fn schedule(_: bool) -> Result<()> {
    Err(Error::Config(
        "Background scheduling requires Windows".into(),
    ))
}

#[cfg(windows)]
pub fn notify(event: &Event, count: usize) -> Result<()> {
    let key = event.item.target.key();
    let url = format!("{URI_PREFIX}{}", if count == 1 { &key } else { "all" });
    let title = if count == 1 {
        format!("Chronver: {}", event.item.title)
    } else {
        format!("Chronver: {count} watch updates")
    };
    let detail = if count == 1 {
        event.label()
    } else {
        format!("{}: {}", event.item.title, event.label())
    };
    powershell(
        include_str!("background_toast.ps1"),
        &[
            ("CHRONVER_WATCH_TITLE", &title),
            ("CHRONVER_WATCH_DETAIL", &detail),
            ("CHRONVER_WATCH_LINK", &url),
        ],
    )
}

#[cfg(not(windows))]
pub fn notify(_: &Event, _: usize) -> Result<()> {
    Err(Error::Config(
        "Desktop notifications require Windows".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notification_links_cannot_supply_worker_commands_or_paths() {
        assert_eq!(activation("chronver-watch:all"), Some("all"));
        assert!(activation(&format!("chronver-watch:{}", "a".repeat(64))).is_some());
        assert_eq!(activation("chronver-watch:--background-watch"), None);
        assert_eq!(activation("chronver-watch:c:/secret"), None);
        assert_eq!(activation("chronver-watch:all --background-watch"), None);
    }
}
