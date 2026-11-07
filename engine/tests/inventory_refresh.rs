use releasescout_engine::{
    Result,
    inventory::{self, CommandOutput, Manager, Runner, Scan},
    inventory_refresh,
};
use std::{cell::RefCell, collections::VecDeque};

struct Fake {
    replies: RefCell<VecDeque<String>>,
    scripts: RefCell<Vec<String>>,
}
impl Fake {
    fn new(replies: Vec<String>) -> Self {
        Self {
            replies: RefCell::new(replies.into()),
            scripts: RefCell::new(vec![]),
        }
    }
}
impl Runner for Fake {
    fn run(&self, script: &str) -> Result<CommandOutput> {
        self.scripts.borrow_mut().push(script.into());
        Ok(CommandOutput {
            code: 0,
            stdout: self
                .replies
                .borrow_mut()
                .pop_front()
                .expect("unexpected command"),
            stderr: String::new(),
        })
    }
}
fn choco(installed: &str, updates: &str) -> String {
    serde_json::json!({"installed":format!("fixture|{installed}"),"updates":updates}).to_string()
}
fn previous() -> Scan {
    let mut scan = inventory::parse(
        Manager::Chocolatey,
        true,
        &choco("1.0.0", "fixture|1.0.0|2.0.0|false"),
    )
    .unwrap();
    scan.candidate_checked_at =
        Some((chrono::Utc::now() - chrono::Duration::minutes(2)).to_rfc3339());
    scan
}

#[test]
fn startup_snapshot_freshness_is_provider_scoped_and_does_not_extend_timestamps() {
    let now = chrono::Utc::now();
    let mut scan = previous();
    scan.checked_at = (now - chrono::Duration::hours(23)).to_rfc3339();
    scan.update_check_succeeded = false;
    scan.packages[0].candidate_check_succeeded = Some(false);
    scan.candidate_checked_at =
        Some((now - chrono::Duration::days(2)).to_rfc3339());
    let before = serde_json::to_vec(&scan).unwrap();
    assert!(inventory_refresh::reusable_snapshot(Manager::Chocolatey, &scan, now));
    assert!(!inventory_refresh::reusable_snapshot(Manager::Scoop, &scan, now));
    assert_eq!(before, serde_json::to_vec(&scan).unwrap());
    for checked_at in [
        (now - chrono::Duration::hours(24)).to_rfc3339(),
        (now + chrono::Duration::minutes(1)).to_rfc3339(),
        "invalid".into(),
    ] {
        scan.checked_at = checked_at;
        assert!(!inventory_refresh::reusable_snapshot(Manager::Chocolatey, &scan, now));
    }
}

#[test]
fn recent_candidates_are_reused_only_after_installed_identity_validation() {
    let old = previous();
    let runner = Fake::new(vec![choco("1.0.0", "")]);
    let scan = inventory_refresh::startup(Manager::Chocolatey, Some(&old), &runner).unwrap();
    assert_eq!(scan.packages[0].available.as_deref(), Some("2.0.0"));
    assert_eq!(scan.candidate_time(), old.candidate_time());
    assert_ne!(scan.checked_at, scan.candidate_time());
    assert!(scan.update_check_succeeded);
    assert_eq!(scan.packages[0].candidate_check_succeeded, Some(true));
    assert_eq!(runner.scripts.borrow().len(), 1);
    assert!(!runner.scripts.borrow()[0].contains("outdated"));
    let runner = Fake::new(vec![
        choco("1.1.0", ""),
        choco("1.1.0", "fixture|1.1.0|2.0.0|false"),
    ]);
    let scan = inventory_refresh::startup(Manager::Chocolatey, Some(&old), &runner).unwrap();
    assert_eq!(runner.scripts.borrow().len(), 2);
    assert!(runner.scripts.borrow()[1].contains("outdated"));
    assert_eq!(scan.packages[0].installed.as_deref(), Some("1.1.0"));
    assert_ne!(scan.candidate_time(), old.candidate_time());
}

#[test]
fn expired_failed_future_or_wrong_provider_checks_cannot_suppress_refresh() {
    for kind in 0..4 {
        let mut old = previous();
        match kind {
            0 => {
                old.candidate_checked_at =
                    Some((chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339())
            }
            1 => old.update_check_succeeded = false,
            2 => {
                old.candidate_checked_at =
                    Some((chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339())
            }
            _ => old.manager = Manager::Npm,
        }
        let runner = Fake::new(vec![choco("1.0.0", "fixture|1.0.0|2.0.0|false")]);
        inventory_refresh::startup(Manager::Chocolatey, Some(&old), &runner).unwrap();
        assert_eq!(runner.scripts.borrow().len(), 1);
        assert!(runner.scripts.borrow()[0].contains("outdated"));
    }
}

#[test]
fn scoop_refresh_precedes_local_status_and_failures_remain_visible() {
    for fresh in [false, true] {
        let runner = Fake::new(vec![serde_json::json!({"fresh":fresh,"action":if fresh {"recent"} else {"failed"},"warnings":if fresh {vec![]} else {vec!["bucket refresh failed"]}}).to_string(),
            serde_json::json!({"installed":{"apps":[{"Name":"fixture","Version":"1.0.0","Source":"main"}]},"updates":[{"Name":"fixture","Installed Version":"1.0.0","Latest Version":"2.0.0"}]}).to_string()]);
        let scan = inventory_refresh::startup(Manager::Scoop, None, &runner).unwrap();
        assert_eq!(
            runner.scripts.borrow()[0],
            inventory_refresh::scoop_refresh_script()
        );
        assert!(runner.scripts.borrow()[1].contains("scoop status"));
        assert_eq!(scan.packages[0].available.as_deref(), Some("2.0.0"));
        assert_eq!(
            scan.warnings
                .iter()
                .any(|w| w.contains("buckets were not refreshed")),
            !fresh
        );
        if !fresh {
            assert!(scan.warnings.iter().any(|w| w == "bucket refresh failed"));
        }
    }
}

#[test]
fn local_only_providers_do_not_get_candidate_or_upgrade_commands() {
    let runner = Fake::new(vec!["[]".into()]);
    let scan = inventory_refresh::startup(Manager::Store, None, &runner).unwrap();
    assert!(!scan.update_check_requested);
    assert_eq!(runner.scripts.borrow().len(), 1);
    for manager in [
        Manager::Store,
        Manager::Local,
        Manager::Vscode,
        Manager::VscodeInsiders,
        Manager::Cursor,
    ] {
        assert!(!inventory_refresh::supports_candidates(manager));
    }
}

#[cfg(windows)]
#[test]
fn scoop_script_recognizes_external_fetches_and_never_upgrades_an_application() {
    for (fresh, dirty, diverged, failed, empty_fetch, expected) in [
        (true, false, false, false, false, "recent"),
        (false, false, false, false, false, "updated"),
        (true, true, false, false, false, "blocked"),
        (true, false, true, false, false, "updated"),
        (false, false, false, true, false, "failed"),
        (true, false, false, true, true, "failed"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().replace('\'', "''");
        let prefix = format!(
            r#"
$root='{root}'
$global:updates=0
$global:dirty={dirty}
$global:diverged={diverged}
$global:failed={failed}
$global:empty_fetch={empty_fetch}
$commit='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
$other='bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
$gitDir=Join-Path $root 'buckets/main/.git'
$null=New-Item -ItemType Directory -Path $gitDir -Force
$fetch=Join-Path $gitDir 'FETCH_HEAD'
$null=New-Item -ItemType File -Path $fetch -Force
if (-not $global:empty_fetch) {{ Set-Content -LiteralPath $fetch -Value @("$other`t`tbranch 'other' of fixture", "$commit`tnot-for-merge`tbranch 'main' of fixture") }}
if (-not {fresh}) {{ (Get-Item -LiteralPath $fetch).LastWriteTimeUtc=[DateTime]::UtcNow.AddHours(-2) }}
function global:scoop {{
    $global:LASTEXITCODE=0
    if ($args[0] -eq 'prefix') {{ return (Join-Path $root 'apps/scoop/current') }}
    if ($args.Count -ne 1 -or $args[0] -ne 'update') {{ throw 'Unexpected Scoop operation' }}
    $global:updates++
    if ($global:failed) {{ if ($global:empty_fetch) {{ throw 'Fixture bucket unavailable' }}; Write-Error 'Fixture bucket unavailable'; return }}
    $global:diverged=$false
    (Get-Item -LiteralPath $fetch).LastWriteTimeUtc=[DateTime]::UtcNow
}}
function global:git.exe {{
    $global:LASTEXITCODE=0
    if ($args[2] -eq 'status') {{ if($global:dirty){{return ' M bucket/fixture.json'}}; return }}
    if ($args[3] -eq '--absolute-git-dir') {{ return $gitDir }}
    if ($args[3] -eq '--git-path') {{ return $fetch }}
    if ($args[4] -eq '@{{upstream}}' -and $global:diverged) {{ return $other }}
    return $commit
}}
"#,
            dirty = if dirty { "$true" } else { "$false" },
            fresh = if fresh { "$true" } else { "$false" },
            diverged = if diverged { "$true" } else { "$false" },
            failed = if failed { "$true" } else { "$false" },
            empty_fetch = if empty_fetch { "$true" } else { "$false" }
        );
        let output = inventory::ProcessRunner::default()
            .run(&format!(
                "{prefix}\n{}\nWrite-Output \"CALLS:$global:updates\"",
                inventory_refresh::scoop_refresh_script()
            ))
            .unwrap();
        assert_eq!(output.code, 0, "{}", output.stderr);
        let report: serde_json::Value =
            serde_json::from_str(output.stdout.lines().find(|l| l.starts_with('{')).unwrap())
                .unwrap();
        assert_eq!(report["action"], expected, "{}", output.stderr);
        assert!(
            output
                .stdout
                .contains(if matches!(expected, "updated" | "failed") {
                    "CALLS:1"
                } else {
                    "CALLS:0"
                })
        );
    }
}
