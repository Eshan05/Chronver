use releasescout_engine::inventory::*;
use serde_json::json;
struct Fake {
    body: String,
    code: i32,
}
#[test]
fn scoop_candidate_with_a_valid_identity_is_not_reported_as_ambiguous() {
    let scan = parse(Manager::Scoop, true, &json!({
        "installed":{"apps":[{"Name":"fixture-tool","Version":"r40","Source":"main"}]},
        "updates":[{"Name":"fixture-tool","Installed Version":"r40","Latest Version":"r42"}]
    }).to_string()).unwrap();
    let record = &scan.packages[0];
    assert_eq!(record.available.as_deref(), Some("r42"));
    assert_eq!(record.candidate_check_succeeded, Some(true));
    assert!(!record.candidate_note.as_deref().unwrap().contains("ambiguous"));
    let failed = parse(Manager::Scoop, true, &json!({
        "installed":{"apps":[{"Name":"fixture-tool","Version":"r40","Source":"main"}]},
        "updates":[{"Name":"fixture-tool","Installed Version":"r39","Latest Version":"r42"}]
    }).to_string()).unwrap();
    assert_eq!(failed.packages[0].candidate_check_succeeded, Some(false));
    assert_eq!(failed.packages[0].available, None);
}
#[test]
fn installed_source_hints_survive_scan_and_snapshot_serialization() {
    let scan=parse(Manager::Pnpm,false,&json!({"root":"C:/global","installed":[{"dependencies":{"@acme/cli":{"version":"1.0.0"}}}],"metadata":{"@acme/cli":{"repository":"https://github.com/acme/suite","repositoryDirectory":"packages/cli","releaseNotes":"https://example.com/changelog","downloads":["https://example.com/tool.zip"],"packageRoot":"C:/global/@acme/cli"}}}).to_string()).unwrap();
    let scan: Scan = serde_json::from_str(&serde_json::to_string(&scan).unwrap()).unwrap();
    let p = &scan.packages[0];
    assert_eq!(
        p.repository.as_deref(),
        Some("https://github.com/acme/suite")
    );
    assert_eq!(p.repository_directory.as_deref(), Some("packages/cli"));
    assert_eq!(
        p.release_notes.as_deref(),
        Some("https://example.com/changelog")
    );
    assert_eq!(p.download_urls.len(), 1);
    assert_eq!(p.package_root.as_deref(), Some("C:/global/@acme/cli"));
}
impl Runner for Fake {
    fn run(&self, command: &str) -> releasescout_engine::Result<CommandOutput> {
        assert!(command.contains("-Compress"));
        Ok(CommandOutput {
            code: self.code,
            stdout: self.body.clone(),
            stderr: "fixture failure".into(),
        })
    }
}
#[test]
fn npm_and_pnpm_keep_scope_and_scoped_names() {
    for manager in [Manager::Npm, Manager::Pnpm] {
        let installed = json!({"dependencies":{"@scope/tool":{"version":"1.0.0"}}});
        let data = json!({"root":"C:/global/node_modules","installed":if manager==Manager::Pnpm {json!([installed])}else{installed},"installedExit":0,"updateExit":1,"updates":{"@scope/tool":{"current":"1.0.0","wanted":"1.1.0","latest":"2.0.0"}}});
        let s = scan(
            manager,
            true,
            &Fake {
                body: data.to_string(),
                code: 0,
            },
        )
        .unwrap();
        assert_eq!(s.packages[0].id, "@scope/tool");
        assert_eq!(s.packages[0].available.as_deref(), Some("2.0.0"));
        assert!(s.update_check_succeeded);
        assert_eq!(
            s.packages[0].root.as_deref(),
            Some("C:/global/node_modules")
        );
    }
}
#[test]
fn partial_candidate_failures_are_scoped_to_the_matching_identity() {
    for manager in [Manager::Npm, Manager::Pnpm] {
        let installed = json!({"dependencies":{
            "current-tool":{"version":"1.0.0"},
            "update-tool":{"version":"1.0.0"},
            "stale-tool":{"version":"2.0.0"},
            "bad-tool":{"version":"1.0.0"}
        }});
        let input = json!({"root":"C:/global","installed":if manager == Manager::Pnpm {json!([installed])} else {installed},
        "installedExit":0,"updateExit":1,"updates":{
            "uninstalled-tool":{"current":"1.0.0","latest":"2.0.0"},
            "update-tool":{"current":"1.0.0","latest":"2.0.0"},
            "stale-tool":{"current":"1.0.0","latest":"3.0.0"},
            "bad-tool":{"latest":"2.0.0"}
        }});
        let scan = parse(manager, true, &input.to_string()).unwrap();
        assert!(!scan.update_check_succeeded);
        assert!(scan.candidate_checked_at.is_some());
        let scan: Scan = serde_json::from_str(&serde_json::to_string(&scan).unwrap()).unwrap();
        let get = |id: &str| scan.packages.iter().find(|p| p.id == id).unwrap();
        for id in ["current-tool", "update-tool"] {
            assert_eq!(get(id).candidate_check_succeeded, Some(true));
            assert!(get(id).candidate_note.is_none());
        }
        assert!(get("current-tool").available.is_none());
        assert_eq!(get("update-tool").available.as_deref(), Some("2.0.0"));
        for id in ["stale-tool", "bad-tool"] {
            assert_eq!(get(id).candidate_check_succeeded, Some(false));
            assert!(get(id).available.is_none());
            assert!(get(id).candidate_note.as_deref().unwrap().contains(id));
        }
        assert!(scan.warnings.iter().any(|w| w.contains("uninstalled-tool")));
        assert!(!scan.packages.iter().any(|p| p.id == "uninstalled-tool"));
    }
}

#[test]
fn provider_wide_candidate_failures_still_fail_each_record_and_old_snapshots_load() {
    let input = json!({"root":"C:/global","installed":{"dependencies":{"one":{"version":"1.0.0"},"two":{"version":"1.0.0"}}},
        "installedExit":0,"updateExit":2,"updates":{"one":{"current":"1.0.0","latest":"2.0.0"}}});
    let scan = parse(Manager::Npm, true, &input.to_string()).unwrap();
    assert!(!scan.update_check_succeeded);
    assert!(
        scan.packages
            .iter()
            .all(|p| p.candidate_check_succeeded == Some(false) && p.available.is_none())
    );
    let mut old = serde_json::to_value(scan).unwrap();
    for p in old["packages"].as_array_mut().unwrap() {
        p.as_object_mut()
            .unwrap()
            .remove("candidate_check_succeeded");
    }
    let old: Scan = serde_json::from_value(old).unwrap();
    assert!(
        old.packages
            .iter()
            .all(|p| p.candidate_check_succeeded.is_none())
    );
    assert!(!old.update_check_succeeded);
}

#[test]
fn raw_pnpm_json_survives_powershell_without_array_reserialization() {
    let installed = json!([{"path":"C:/global","private":true,"dependencies":{"@scope/tool":{"version":"1.2.3"}}}]);
    let data = json!({"root":"C:/global","installedJson":installed.to_string(),"installedExit":0,"updatesJson":"{}","updateExit":0});
    let s = parse(Manager::Pnpm, true, &data.to_string()).unwrap();
    assert_eq!(s.packages[0].installed.as_deref(), Some("1.2.3"));
    assert!(s.update_check_succeeded);
    assert!(script(Manager::Pnpm, false).contains("installedJson=$installed"));
    assert!(script(Manager::Pnpm, false).contains("$installed=($i|Out-String)"));
    assert!(!script(Manager::Pnpm, false).contains("$installed=($i|Out-String|ConvertFrom-Json)"));
}

#[test]
fn scoop_distinguishes_omitted_healthy_rows_from_reported_problems() {
    let s=parse(Manager::Scoop,true,&json!({"installed":{"apps":[{"Name":"current","Version":"1.0"},{"Name":"removed","Version":"2.0"},{"Name":"old","Version":"1.0"}]},"updates":[{"Name":"removed","Installed Version":"2.0","Latest Version":"","Info":"Manifest removed"},{"Name":"old","Installed Version":"1.0","Latest Version":"2.0"}]}).to_string()).unwrap();
    assert_eq!(
        s.packages[0].candidate_note.as_deref(),
        Some("No newer version in local buckets")
    );
    assert!(
        s.packages[1]
            .candidate_note
            .as_ref()
            .unwrap()
            .contains("Manifest removed")
    );
    assert!(s.packages[1].available.is_none());
    assert_eq!(s.packages[2].available.as_deref(), Some("2.0"));
    let s = parse(
        Manager::Scoop,
        false,
        &json!({"installed":{"apps":[{"Name":"current","Version":"1.0"}]}}).to_string(),
    )
    .unwrap();
    assert!(s.packages[0].candidate_note.is_none());
}

#[test]
fn insiders_extensions_never_fall_back_to_stable_or_invent_candidates() {
    let plan = script(Manager::VscodeInsiders, true);
    assert!(plan.contains("Get-Command code-insiders.cmd"));
    assert!(plan.contains("'--list-extensions','--show-versions'"));
    assert!(plan.contains("$state.userDataProfiles"));
    assert!(plan.contains("'--profile='+$profile.name"));
    assert!(!plan.contains("& code.cmd"));
    assert!(!plan.contains("--update-extensions"));
    let s = parse(
        Manager::VscodeInsiders,
        true,
        &json!({"installed":"publisher.tool@1.0.0\nother.extension@2.0.0-pre.1\n"}).to_string(),
    )
    .unwrap();
    assert_eq!(s.packages.len(), 2);
    assert_eq!(s.packages[1].installed.as_deref(), Some("2.0.0-pre.1"));
    assert!(!s.update_check_succeeded);
    assert!(s.packages.iter().all(|p| p.available.is_none()));
    assert!(
        parse(
            Manager::VscodeInsiders,
            false,
            &json!({"installed":"Warning: failure"}).to_string()
        )
        .is_err()
    );
}

#[test]
fn editor_profiles_keep_explicit_identities_and_versions_across_sources() {
    for manager in Manager::EDITORS {
        let plan = script(manager, true);
        assert!(!plan.contains("--update-extensions"));
        assert!(!plan.contains("--install-extension"));
        let scan = parse(manager, true, &json!({"profiles":[
            {"id":"a","name":"Work","icon":"vm","root":"C:/editor/profiles/a","installed":"vendor.tool@1.0.0"},
            {"id":"b","name":"Work","root":"C:/editor/profiles/b","installed":"vendor.tool@2.0.0"}
        ]}).to_string()).unwrap();
        assert_eq!(scan.packages.len(), 2);
        for (i, p) in scan.packages.iter().enumerate() {
            assert_eq!(p.manager, manager);
            let info = p.extension.as_ref().unwrap();
            assert_eq!(
                info.profile_id.as_deref(),
                Some(if i == 0 { "a" } else { "b" })
            );
            assert_eq!(info.profile_name.as_deref(), Some("Work"));
            assert_eq!(info.profile_icon.as_deref(), if i == 0 { Some("vm") } else { None });
            let roundtrip: releasescout_engine::inventory::Scan = serde_json::from_str(&serde_json::to_string(&scan).unwrap()).unwrap();
            assert_eq!(roundtrip.packages[i].extension.as_ref().unwrap().profile_icon, info.profile_icon);
            assert!(p.available.is_none());
        }
        assert_eq!(scan.packages[1].installed.as_deref(), Some("2.0.0"));
        assert!(!scan.update_check_succeeded);
    }
    assert!(script(Manager::Vscode, false).contains("$editorCommand='code.cmd'"));
    assert!(script(Manager::Cursor, false).contains("$editorCommand='cursor.cmd'"));
}

#[test]
fn invalid_or_unknown_profile_icons_cannot_be_interpreted_as_resources() {
    for (icon, expected) in [(json!("custom-future-icon"),Some("custom-future-icon")),(json!("file:///C:/private/icon.svg"),None),(json!("https://example.com/icon.svg"),None),(json!("bad\nicon"),None),(json!(""),None),(json!("x".repeat(97)),None),(json!({"id":"vm"}),None)] {
        let scan = parse(Manager::VscodeInsiders,false,&json!({"profiles":[{"id":"work","name":"Work","icon":icon,"installed":"vendor.tool@1.0.0"}]}).to_string()).unwrap();
        assert_eq!(scan.packages[0].extension.as_ref().unwrap().profile_icon.as_deref(),expected);
    }
}

#[test]
fn bun_uses_global_listing_and_recognizes_an_empty_global_root() {
    assert!(script(Manager::Bun, false).contains("pm ls -g"));
    let s=parse(Manager::Bun,true,&json!({"installed":"C:/Bun/global node_modules (0 installed)\n","updates":"","updateExit":0}).to_string()).unwrap();
    assert!(s.packages.is_empty());
    assert!(s.update_check_succeeded);
}

#[test]
fn scoop_keeps_buckets_holds_and_ambiguous_scopes() {
    let data = json!({"installed":{"apps":[{"Name":"tool","Version":"1.0","Source":"main","Info":"Held package"},{"Name":"tool","Version":"1.0","Source":"extras","Info":"Global install"}]},"updates":[{"Name":"tool","Installed Version":"1.0","Latest Version":"2.0"}]});
    let s = parse(Manager::Scoop, true, &data.to_string()).unwrap();
    assert_eq!(s.packages.len(), 2);
    assert!(s.packages[0].pinned);
    assert_eq!(s.packages[1].scope, "machine");
    assert!(s.packages.iter().all(|p| p.available.is_none()));
    assert!(!s.update_check_succeeded);
    assert!(script(Manager::Scoop, true).contains("status -l"));
}
#[test]
fn choco_limited_output_does_not_swallow_warnings_as_packages() {
    let s = parse(
        Manager::Chocolatey,
        true,
        &json!({"installed":"git|2.0\n","updates":"git|2.0|3.0|false\n"}).to_string(),
    )
    .unwrap();
    assert_eq!(s.packages[0].available.as_deref(), Some("3.0"));
    assert!(
        parse(
            Manager::Chocolatey,
            false,
            &json!({"installed":"Error while listing"}).to_string()
        )
        .is_err()
    );
    assert!(script(Manager::Chocolatey, false).contains("--local-only"));
}
#[test]
fn winget_export_is_observation_not_install_origin() {
    let s=parse(Manager::Winget,false,&json!({"installed":{"Sources":[{"SourceDetails":{"Name":"msstore"},"Packages":[{"PackageIdentifier":"9NTEST","Version":"Unknown"}]}]}}).to_string()).unwrap();
    assert_eq!(s.packages[0].source, "msstore");
    assert!(s.packages[0].installed.is_none());
    assert!(!s.packages[0].install_origin_known);
    let s = parse(
        Manager::Winget,
        true,
        &json!({"installed":{"Sources":[]},"updates":"unrecognized locale","updateExit":0})
            .to_string(),
    )
    .unwrap();
    assert!(!s.update_check_succeeded);
    assert!(!s.warnings.is_empty());
}
#[test]
fn appx_registration_does_not_imply_store_update_coverage() {
    let s=parse(Manager::Store,true,&json!([{"Name":"Example","PackageFamilyName":"Example_pub","Version":"1.2.3.4","InstallLocation":"C:/WindowsApps/Example"}]).to_string()).unwrap();
    assert_eq!(s.packages[0].id, "Example_pub");
    assert!(!s.packages[0].install_origin_known);
    assert!(!s.update_check_succeeded);
    assert!(s.packages[0].available.is_none());
    assert!(
        parse(Manager::Store, false, "[]")
            .unwrap()
            .packages
            .is_empty()
    );
}
#[test]
fn bun_handles_scoped_packages_and_latest_table_column() {
    let body = json!({"root":"C:/Bun/global","installed":"global node_modules (1)\n\u{2514}\u{2500}\u{2500} @scope/tool@1.0.0\n","updates":"| Package | Current | Update | Latest |\n| --- | --- | --- | --- |\n| @scope/tool | 1.0.0 | 1.1.0 | 2.0.0 |","updateExit":0});
    let s = parse(Manager::Bun, true, &body.to_string()).unwrap();
    assert_eq!(s.packages[0].id, "@scope/tool");
    assert_eq!(s.packages[0].available.as_deref(), Some("2.0.0"));
    assert!(
        parse(
            Manager::Bun,
            false,
            &json!({"root":"C:/x","installed":"error"}).to_string()
        )
        .is_err()
    );
}
#[test]
fn failed_commands_bad_json_and_races_are_not_empty_success() {
    assert!(
        scan(
            Manager::Npm,
            false,
            &Fake {
                body: "{}".into(),
                code: 1
            }
        )
        .is_err()
    );
    assert!(parse(Manager::Npm, false, "bad JSON").is_err());
    let s=parse(Manager::Npm,true,&json!({"root":"C:/global","installed":{"dependencies":{"tool":{"version":"2.0"}}},"installedExit":0,"updateExit":1,"updates":{"tool":{"current":"1.0","latest":"3.0"}}}).to_string()).unwrap();
    assert!(s.packages[0].available.is_none());
    assert!(!s.update_check_succeeded);
}
#[test]
fn command_plans_never_install_upgrade_or_refresh_sources() {
    for manager in Manager::ALL {
        for updates in [false, true] {
            let plan = script(manager, updates);
            for forbidden in [
                " install ",
                " upgrade ",
                "source update",
                "scoop update",
                "Install-Module",
                "--accept-source-agreements",
            ] {
                assert!(!plan.contains(forbidden), "{manager:?}: {forbidden}");
            }
            assert!(plan.contains("OutputEncoding"));
        }
    }
}

#[cfg(windows)]
#[test]
fn every_command_plan_has_valid_powershell_syntax_without_running_it() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let plans: Vec<_> = Manager::ALL
        .into_iter()
        .flat_map(|manager| {
            [false, true].map(move |updates| {
                serde_json::json!({
                    "name": format!("{} updates={updates}", manager.id()),
                    "script": script(manager, updates)
                })
            })
        })
        .collect();
    // ParseInput builds an AST only. None of the manager commands are evaluated.
    let check = "$ErrorActionPreference='Stop'; $plans=[Console]::In.ReadToEnd()|ConvertFrom-Json; foreach($p in $plans) {$tokens=$null;$errors=$null;$null=[Management.Automation.Language.Parser]::ParseInput($p.script,[ref]$tokens,[ref]$errors);if($errors.Count){Write-Output $p.name;Write-Output $errors;exit 1}}";
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            check,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&plans).unwrap())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
