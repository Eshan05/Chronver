//! Startup candidate checks and source metadata refreshes, never package upgrades.
use crate::{
    Result,
    inventory::{self, Manager, Runner, Scan},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

const CANDIDATE_TTL: i64 = 30 * 60;
pub const AUTOMATIC_SCAN_INTERVAL: chrono::Duration = chrono::Duration::hours(24);
static SCOOP_REFRESH: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn supports_candidates(manager: Manager) -> bool {
    matches!(
        manager,
        Manager::Scoop
            | Manager::Chocolatey
            | Manager::Winget
            | Manager::Npm
            | Manager::Pnpm
            | Manager::Bun
    )
}

pub fn reusable_snapshot(manager: Manager, scan: &Scan, now: DateTime<Utc>) -> bool {
    scan.manager == manager
        && DateTime::parse_from_rfc3339(&scan.checked_at)
            .is_ok_and(|time| recent_scan_attempt(time.into(), now))
}

pub fn recent_scan_attempt(at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    (chrono::Duration::zero()..AUTOMATIC_SCAN_INTERVAL).contains(&(now - at))
}

fn recent(scan: &Scan) -> bool {
    scan.update_check_requested
        && scan.update_check_succeeded
        && DateTime::parse_from_rfc3339(scan.candidate_time()).is_ok_and(|time| {
            (0..CANDIDATE_TTL).contains(&(Utc::now().timestamp() - time.timestamp()))
        })
}

fn signature(scan: &Scan) -> Vec<String> {
    let mut entries: Vec<_> = scan
        .packages
        .iter()
        .map(|p| {
            serde_json::to_string(&(&p.id, &p.installed, &p.source, &p.scope, &p.root, p.pinned))
                .expect("serializable inventory identity")
        })
        .collect();
    entries.sort();
    entries
}

#[derive(Deserialize)]
struct ScoopRefresh {
    fresh: bool,
    action: String,
    warnings: Vec<String>,
}

pub fn scoop_refresh_script() -> &'static str {
    include_str!("inventory/scoop-refresh.ps1")
}

pub fn check(manager: Manager, runner: &impl Runner) -> Result<Scan> {
    let refresh = (manager == Manager::Scoop).then(|| {
        let _guard = SCOOP_REFRESH.lock().map_err(|_| {
            crate::Error::Source(
                "Previous Scoop refresh worker failed; metadata refresh skipped".into(),
            )
        })?;
        runner.run(scoop_refresh_script()).and_then(|output| {
            if output.code != 0 {
                return Err(crate::Error::Source(format!(
                    "Scoop metadata refresh failed: {}",
                    output.stderr.trim()
                )));
            }
            serde_json::from_str::<ScoopRefresh>(&output.stdout)
                .map_err(|e| crate::Error::Source(format!("Invalid Scoop refresh report: {e}")))
        })
    });
    let mut scan = inventory::scan(manager, true, runner)?;
    if let Some(refresh) = refresh {
        match refresh {
            Ok(refresh) => {
                if refresh.fresh && matches!(refresh.action.as_str(), "recent" | "updated") {
                    scan.warnings.retain(|w| w != "Scoop candidates use local bucket manifests; buckets were not refreshed");
                }
                if !refresh.fresh
                    && let Some(detail) = refresh.warnings.iter().find(|w| {
                        w.starts_with("Stale or unavailable sources:")
                            || w.starts_with("Affected sources:")
                    })
                {
                    scan.warnings
                        .insert(0, format!("Scoop metadata refresh incomplete. {detail}"));
                }
                scan.warnings.extend(refresh.warnings);
            }
            Err(error) => scan
                .warnings
                .insert(0, error.to_string().chars().take(1000).collect()),
        }
    }
    Ok(scan)
}

pub fn startup(manager: Manager, previous: Option<&Scan>, runner: &impl Runner) -> Result<Scan> {
    if !supports_candidates(manager) {
        return inventory::scan(manager, false, runner);
    }
    // Scoop status is local and cheap; always recompute after checking bucket freshness.
    if manager != Manager::Scoop
        && let Some(old) = previous.filter(|s| s.manager == manager && recent(s))
    {
        let mut installed = inventory::scan(manager, false, runner)?;
        if signature(&installed) == signature(old) {
            for record in &mut installed.packages {
                if let Some(cached) = old.packages.iter().find(|p| {
                    p.id == record.id
                        && p.source == record.source
                        && p.scope == record.scope
                        && p.root == record.root
                        && p.installed == record.installed
                }) {
                    record.available = cached.available.clone();
                    record.candidate_check_succeeded = Some(
                        cached
                            .candidate_check_succeeded
                            .unwrap_or(old.update_check_succeeded),
                    );
                    record.candidate_note = cached.candidate_note.clone();
                }
            }
            installed.update_check_requested = true;
            installed.update_check_succeeded = true;
            installed.candidate_checked_at = Some(old.candidate_time().into());
            for warning in &old.warnings {
                if !installed.warnings.contains(warning) {
                    installed.warnings.push(warning.clone());
                }
            }
            return Ok(installed);
        }
    }
    check(manager, runner)
}
