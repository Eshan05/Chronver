use std::process::Command;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_chronver"))
        .args(args)
        .env_remove("GITHUB_TOKEN")
        .output()
        .unwrap()
}
#[test]
fn cli_uses_chronver_branding() {
    let help = cli(&["--help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("Usage: chronver"));
    assert!(!help.contains("releasescout-engine"));
    let version = cli(&["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        concat!("chronver ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn cache_prune_previews_by_default_and_only_removes_raw_responses() {
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join(format!("{}.json", "a".repeat(64)));
    let saved = dir.path().join(format!("{}.json", "b".repeat(64)));
    std::fs::write(
        &raw,
        r#"{"url":"https://example.com/releases","fetched_at":"2026-01-01T00:00:00Z","headers":{},"body":"Notes"}"#,
    )
    .unwrap();
    std::fs::write(&saved, r#"{"notes":["Keep me"]}"#).unwrap();
    let missing_catalog = dir.path().join("missing.json");
    let args = [
        "cache",
        "prune",
        "--max-mib",
        "0",
        "--cache-dir",
        dir.path().to_str().unwrap(),
        "--catalog",
        missing_catalog.to_str().unwrap(),
    ];
    let preview = cli(&args);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(report["planned_files"], 1);
    assert_eq!(report["removed_files"], 0);
    assert!(raw.exists());

    let mut args = args.to_vec();
    args.push("--apply");
    let result = cli(&args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["removed_files"], 1);
    assert!(!raw.exists() && saved.exists());
}
#[test]
fn catalog_is_machine_readable_and_complete() {
    let output = cli(&["catalog"]);
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["packages"].as_array().unwrap().len() >= 139);
}

#[test]
fn documented_source_overlay_validates_without_fetching() {
    let overlay = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/examples/catalog-overrides.json"
    );
    let output = cli(&["--offline", "--catalog-overrides", overlay, "catalog"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let gh = json["packages"].as_array().unwrap().iter().find(|p| p["id"] == "gh").unwrap();
    assert_eq!(gh["streams"].as_array().unwrap().len(), 1);
    assert_eq!(gh["streams"][0]["scope"], "GitHub CLI stable releases");
    assert!(json["packages"].as_array().unwrap().iter().any(|p| p["id"] == "podman"));
}
#[test]
fn inventory_plan_can_be_reviewed_without_running_a_manager() {
    let output = cli(&["scan", "--manager", "npm", "--check-updates", "--show-plan"]);
    assert!(output.status.success());
    let data: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let script = data["script"].as_str().unwrap();
    assert!(script.contains("npm.cmd list -g"));
    assert!(script.contains("npm.cmd outdated -g"));
    assert!(!script.contains("$exe"));
}
#[test]
fn invalid_queries_and_missing_profiles_are_actionable() {
    for (args, expected) in [
        (
            vec![
                "range",
                "gh",
                "--from",
                "2.0.0",
                "--to",
                "1.0.0",
                "--offline",
            ],
            "from < to",
        ),
        (vec!["inspect", "podman", "--offline"], "--component"),
        (vec!["inspect", "vsc", "--offline"], "not configured"),
        (vec!["inspect", "easiercommit", "--offline"], "not configured"),
        (
            vec![
                "range",
                "notion",
                "--from",
                "1.0.0",
                "--to",
                "2.0.0",
                "--offline",
            ],
            "dated product feeds",
        ),
    ] {
        let output = cli(&args);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
}
#[test]
fn source_failure_still_writes_a_json_report() {
    let temp = tempfile::tempdir().unwrap();
    let output_path = temp.path().join("report.json");
    let output = cli(&[
        "inspect",
        "gh",
        "--offline",
        "--cache-dir",
        temp.path().to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output_path).unwrap()).unwrap();
    assert_eq!(json["status"], "source_failed");
    assert!(
        json["attempts"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("offline cache miss")
    );
}

#[test]
fn batch_keeps_errors_and_source_failures_machine_readable() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("batch.json");
    std::fs::write(&input, serde_json::json!({"schema_version":1,"queries":[
        {"key":"bad","package":"missing-product","query":{"kind":"range","from":"1.0.0","to":"1.1.0"}},
        {"key":"offline","package":"gh","query":{"kind":"range","from":"1.0.0","to":"1.1.0"}}
    ]}).to_string()).unwrap();
    let output = cli(&["batch", "--input", input.to_str().unwrap(), "--offline", "--cache-dir", temp.path().to_str().unwrap(), "--jobs", "2"]);
    assert_eq!(output.status.code(), Some(2));
    let data: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["results"][0]["key"], "bad");
    assert!(data["results"][0]["error"].is_string());
    assert_eq!(data["results"][1]["report"]["status"], "source_failed");
    assert_eq!(data["complete"], false);
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1, "Batch stdout is compact JSON without progress noise");
}

#[test]
fn batch_accepts_stdin_and_rejects_invalid_limits() {
    use std::{io::Write, process::Stdio};
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_chronver"))
        .args(["batch", "--input", "-", "--offline", "--cache-dir", temp.path().to_str().unwrap()])
        .env_remove("GITHUB_TOKEN").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(br#"{"schema_version":1,"queries":[{"key":"q","package":"missing","query":{"kind":"history"}}]}"#).unwrap();
    let result = child.wait_with_output().unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(serde_json::from_slice::<serde_json::Value>(&result.stdout).is_ok());
    let input = temp.path().join("batch.json");
    std::fs::write(&input, br#"{"schema_version":1,"queries":[{"key":"q","package":"gh","query":{"kind":"history"}}]}"#).unwrap();
    let output = cli(&["batch", "--input", input.to_str().unwrap(), "--offline", "--body-bytes", "0"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}
