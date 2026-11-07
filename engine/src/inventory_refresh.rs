//! Installed snapshots and explicit candidate checks, never package upgrades.
use crate::{
    Result,
    inventory::{self, InstalledPackage, Manager, Runner, Scan},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

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

fn installed_identity(
    p: &InstalledPackage,
) -> (&str, &str, &str, Option<&str>, Option<&str>, bool) {
    (
        &p.id,
        &p.source,
        &p.scope,
        p.root.as_deref(),
        p.installed.as_deref(),
        p.pinned,
    )
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
    let mut installed = inventory::scan(manager, false, runner)?;
    let Some(old) = previous.filter(|old| {
        supports_candidates(manager)
            && old.manager == manager
            && old.update_check_requested
            && DateTime::parse_from_rfc3339(old.candidate_time())
                .is_ok_and(|time| time <= Utc::now())
    }) else {
        return Ok(installed);
    };

    let mut candidates = std::collections::HashMap::new();
    for record in &old.packages {
        // Duplicate identities cannot establish which old result belongs here.
        candidates
            .entry(installed_identity(record))
            .and_modify(|known| *known = None)
            .or_insert(Some(record));
    }
    let matches: Vec<_> = installed
        .packages
        .iter()
        .map(|record| candidates.remove(&installed_identity(record)).flatten())
        .collect();
    let mut carried = 0;
    for (record, previous) in installed.packages.iter_mut().zip(matches) {
        let Some(previous) = previous.filter(|p| {
            p.candidate_check_succeeded
                .unwrap_or(old.update_check_succeeded)
        }) else {
            continue;
        };
        record.available = previous.available.clone();
        record.candidate_check_succeeded = Some(true);
        record.candidate_note = previous.candidate_note.clone();
        carried += 1;
    }
    if carried > 0 {
        installed.update_check_requested = true;
        installed.update_check_succeeded = carried == installed.packages.len();
        installed.candidate_checked_at = Some(old.candidate_time().into());
        installed.candidate_results_reused = true;
    }
    Ok(installed)
}
