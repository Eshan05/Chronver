use releasescout_engine::{
    extensions::{self, Gallery, Installed, Mode},
    http::{CacheOptions, Document, Fetcher, HttpClient},
    *,
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};
#[derive(Default)]
struct Fake {
    pages: HashMap<String, String>,
    query: Value,
    calls: RefCell<Vec<String>>,
}
fn doc(url: &str, body: String) -> Document {
    Document {
        body,
        headers: Default::default(),
        evidence: Evidence {
            url: url.into(),
            fetched_at: "2026-11-11T00:00:00Z".into(),
            cache: "fixture".into(),
            content_sha256: None,
        },
    }
}
impl Fetcher for Fake {
    fn get(&self, url: &str) -> Result<Document> {
        self.calls.borrow_mut().push(url.into());
        self.pages
            .get(url)
            .cloned()
            .map(|body| doc(url, body))
            .ok_or_else(|| Error::Source("fixture missing".into()))
    }
}
impl Gallery for Fake {
    fn query(&self, id: &str) -> Result<Document> {
        self.calls.borrow_mut().push(format!("query:{id}"));
        Ok(doc(
            "https://marketplace.visualstudio.com/",
            self.query.to_string(),
        ))
    }
}
fn publication(version: &str, preview: bool, platform: &str, url: &str) -> Value {
    json!({"version":version,"targetPlatform":platform,"properties":[{"key":"Microsoft.VisualStudio.Code.PreRelease","value":preview.to_string()}],"files":[{"assetType":"Microsoft.VisualStudio.Services.Content.Changelog","source":url}]})
}
fn query(versions: Vec<Value>) -> Value {
    json!({"results":[{"extensions":[{"publisher":{"publisherName":"Vendor"},"extensionName":"tool","versions":versions}]}]})
}
const CHANGELOG: &str = "# Changes\n## Unreleased\nFuture unshipped work\n## v2.0.0 - 2026-11-01\nFixed a real problem.\n## v1.0.0\nInitial authored changes.\n";
#[test]
fn linked_version_headings_and_legacy_h1_notices_remain_structured() {
    let url = "https://p.gallery.vsassets.io/log";
    let fetch = Fake {
        query: query(vec![publication("2.0.0", false, "universal", url)]),
        pages: HashMap::from([(url.into(), "# Change Log\n## [v2.0.0](https://github.com/vendor/tool/releases/tag/v2.0.0) - 2026-11-01\n## What's Changed\n* Fixed the database connection.\n# Important Information\nA migration notice for this release.\n## [v1.0.0](https://github.com/vendor/tool/releases/tag/v1.0.0)\nInitial release.".into())]),
        ..Default::default()
    };
    let r = extensions::notes(
        &fetch,
        "x",
        "vendor.tool",
        "1.0.0",
        &Installed::default(),
        Mode::Published,
        Channel::Stable,
    )
    .unwrap();
    assert_eq!(r.notes.len(), 1);
    assert_eq!(r.notes[0].version.as_deref(), Some("2.0.0"));
    assert!(r.notes[0].body.contains("migration notice"));
    assert!(r.notes[0].body.contains("database connection"));
    assert!(
        !r.warnings
            .iter()
            .any(|w| w.contains("No versioned sections"))
    );
}

#[test]
fn pointer_changelog_falls_through_to_identity_verified_extension_release() {
    let root = "https://raw.githubusercontent.com/vendor/repo/HEAD/packages/tool/";
    let asset = "https://p.gallery.vsassets.io/log";
    let pointer =
        "# Change Log\n[Click here for release notes](https://github.com/other/engine/releases)";
    let info = Installed {
        repository: Some("https://github.com/vendor/repo.git".into()),
        directory: Some("packages/tool".into()),
        ..Default::default()
    };
    let mut fetch = Fake {
        query: query(vec![publication("2.0.0", false, "universal", asset)]),
        pages: HashMap::from([
            (asset.into(), pointer.into()),
            (format!("{root}package.json"), json!({"publisher":"vendor","name":"tool","version":"2.0.0"}).to_string()),
            (format!("{root}CHANGELOG.md"), pointer.into()),
            ("https://api.github.com/repos/vendor/repo/releases/tags/2.0.0".into(), json!({"tag_name":"2.0.0","name":"Extension 2.0.0","prerelease":false,"body":"Fixed completion in the editor.","published_at":"2026-11-01T00:00:00Z"}).to_string()),
            ("https://raw.githubusercontent.com/vendor/repo/2.0.0/packages/tool/package.json".into(), json!({"publisher":"vendor","name":"tool","version":"2.0.0"}).to_string()),
        ]), ..Default::default()
    };
    let r = extensions::notes(
        &fetch,
        "x",
        "vendor.tool",
        "1.0.0",
        &info,
        Mode::Published,
        Channel::Stable,
    )
    .unwrap();
    assert_eq!(r.notes[0].version.as_deref(), Some("2.0.0"));
    assert!(r.notes[0].authored_notes);
    assert!(r.scope.contains("tagged manifest verified"));
    assert!(
        !fetch
            .calls
            .borrow()
            .iter()
            .any(|u| u.contains("other/engine"))
    );
    fetch.pages.insert(
        "https://api.github.com/repos/vendor/repo/releases/tags/2.0.0".into(),
        json!({"tag_name":"2.0.0","prerelease":false,"body":null}).to_string(),
    );
    let empty = extensions::notes(
        &fetch,
        "x",
        "vendor.tool",
        "1.0.0",
        &info,
        Mode::Published,
        Channel::Stable,
    )
    .unwrap();
    assert_eq!(empty.status, "no_authored_notes");
    assert_eq!(empty.notes[0].version.as_deref(), Some("2.0.0"));
    assert!(!empty.notes[0].authored_notes);
    assert!(empty.notes[0].body.is_empty());
    fetch.pages.insert(
        "https://raw.githubusercontent.com/vendor/repo/2.0.0/packages/tool/package.json".into(),
        json!({"publisher":"vendor","name":"different","version":"2.0.0"}).to_string(),
    );
    let r = extensions::notes(
        &fetch,
        "x",
        "vendor.tool",
        "1.0.0",
        &info,
        Mode::Published,
        Channel::Stable,
    )
    .unwrap();
    assert_eq!(r.status, "no_authored_notes");
    assert_eq!(r.notes[0].title, "Release-notes link");
}

#[test]
fn arbitrary_preview_tag_prefix_is_discovered_and_verified_not_hardcoded() {
    let root = "https://raw.githubusercontent.com/vendor/repo/HEAD/";
    let info = Installed {
        repository: Some("https://github.com/vendor/repo".into()),
        pre_release: Some(true),
        ..Default::default()
    };
    let fetch=Fake { pages:HashMap::from([
        (format!("{root}package.json"),json!({"publisher":"vendor","name":"tool","version":"2.0.0"}).to_string()),
        ("https://api.github.com/repos/vendor/repo/releases?per_page=20".into(),json!([
            {"tag_name":"2.0.0","prerelease":false,"body":"Stable notes must not leak into preview."},
            {"tag_name":"custom-preview/2.0.0","prerelease":true,"body":"Fixed the preview extension completion."}
        ]).to_string()),
        ("https://raw.githubusercontent.com/vendor/repo/custom-preview%2F2.0.0/package.json".into(),json!({"publisher":"vendor","name":"tool","version":"2.0.0"}).to_string()),
    ]),..Default::default() };
    let r = extensions::notes(
        &fetch,
        "x",
        "vendor.tool",
        "2.0.0",
        &info,
        Mode::Installed,
        Channel::Preview,
    )
    .unwrap();
    assert_eq!(r.notes.len(), 1);
    assert!(r.notes[0].preview);
    assert!(r.notes[0].body.contains("preview extension"));
}

#[test]
#[ignore = "explicit live extension recovery check"]
fn live_prisma_and_mongodb() {
    let dir = tempfile::tempdir().unwrap();
    let fetch = HttpClient::new(CacheOptions {
        directory: dir.path().into(),
        public_only: true,
        ..Default::default()
    })
    .unwrap();
    for (id, version, repository, directory) in [
        (
            "prisma.prisma",
            "31.11.0",
            "https://github.com/prisma/language-tools.git",
            Some("packages/vscode"),
        ),
        (
            "mongodb.mongodb-vscode",
            "1.16.0",
            "https://github.com/mongodb-js/vscode",
            None,
        ),
    ] {
        let info = Installed {
            repository: Some(repository.into()),
            directory: directory.map(String::from),
            ..Default::default()
        };
        let r = extensions::notes(
            &fetch,
            id,
            id,
            version,
            &info,
            Mode::Installed,
            Channel::Stable,
        )
        .unwrap();
        assert!(
            r.notes
                .iter()
                .any(|n| n.version.as_deref() == Some(version)),
            "{id}: {r:?}"
        );
        if id.starts_with("mongodb") {
            assert!(r.notes.iter().any(|n| n.authored_notes));
        }
        assert!(
            r.notes
                .iter()
                .all(|n| n.title != "Unstructured extension changelog")
        );
        println!("{id}: {} / {} notes", r.scope, r.notes.len());
    }
}
#[test]
fn installed_changelog_is_offline_version_bound_and_revalidated() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"publisher":"vendor","name":"tool","version":"1.0.0"}"#,
    )
    .unwrap();
    let path = dir.path().join("CHANGELOG.md");
    std::fs::write(&path, CHANGELOG).unwrap();
    let info = Installed {
        root: Some(dir.path().into()),
        changelog: Some(path),
        ..Default::default()
    };
    let fetch = Fake::default();
    let r = extensions::notes(
        &fetch,
        "observation",
        "vendor.tool",
        "1.0.0",
        &info,
        Mode::Installed,
        Channel::Stable,
    )
    .unwrap();
    assert_eq!(r.notes.len(), 1);
    assert_eq!(r.notes[0].version.as_deref(), Some("1.0.0"));
    assert!(!r.complete);
    assert!(fetch.calls.borrow().is_empty());
    assert_eq!(r.evidence[0].cache, "installed_file");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"publisher":"vendor","name":"tool","version":"2.0.0"}"#,
    )
    .unwrap();
    assert!(
        extensions::notes(
            &fetch,
            "observation",
            "vendor.tool",
            "1.0.0",
            &info,
            Mode::Installed,
            Channel::Stable
        )
        .is_err()
    );
}
#[test]
fn publications_preserve_channel_platform_and_identity() {
    let data = query(vec![
        publication(
            "5.0.0",
            true,
            "universal",
            "https://p.gallery.vsassets.io/preview",
        ),
        publication(
            "3.0.0",
            false,
            "linux-x64",
            "https://p.gallery.vsassets.io/linux",
        ),
        publication(
            "2.0.0",
            false,
            "win32-x64",
            "https://p.gallery.vsassets.io/native",
        ),
        publication(
            "2.0.0",
            false,
            "universal",
            "https://p.gallery.vsassets.io/universal",
        ),
    ]);
    let info = Installed {
        target_platform: Some("win32-x64".into()),
        ..Default::default()
    };
    let chosen = extensions::marketplace_asset(
        &data,
        "vendor.tool",
        "1.0.0",
        &info,
        Mode::Published,
        Channel::Stable,
    )
    .unwrap();
    assert_eq!(chosen.0, "https://p.gallery.vsassets.io/native");
    assert_eq!(
        extensions::marketplace_asset(
            &data,
            "vendor.tool",
            "1.0.0",
            &info,
            Mode::Published,
            Channel::Preview
        )
        .unwrap()
        .1,
        "5.0.0"
    );
    assert!(
        extensions::marketplace_asset(
            &data,
            "other.tool",
            "1.0.0",
            &info,
            Mode::Published,
            Channel::Stable
        )
        .is_err()
    );
    assert!(
        extensions::marketplace_asset(
            &data,
            "vendor.tool",
            "1.0.0",
            &info,
            Mode::Installed,
            Channel::Stable
        )
        .is_err()
    );
}
#[test]
fn newer_notes_are_not_claimed_as_available_updates_or_complete_history() {
    let url = "https://p.gallery.vsassets.io/log";
    let fetch = Fake {
        query: query(vec![publication("2.0.0", false, "universal", url)]),
        pages: HashMap::from([(url.into(), CHANGELOG.into())]),
        ..Default::default()
    };
    let r = extensions::notes(
        &fetch,
        "profile-a",
        "vendor.tool",
        "1.0.0",
        &Installed::default(),
        Mode::Published,
        Channel::Stable,
    )
    .unwrap();
    assert_eq!(r.package, "profile-a");
    assert_eq!(r.notes.len(), 1);
    assert_eq!(r.notes[0].version.as_deref(), Some("2.0.0"));
    assert_eq!(r.notes[0].date.unwrap().to_string(), "2026-11-01");
    assert!(!r.complete);
    assert!(r.warnings.iter().any(|w| w.contains("compatibility")));
}
#[test]
fn repository_fallback_must_verify_the_extension_not_a_shared_monorepo() {
    let root = "https://raw.githubusercontent.com/vendor/repo/HEAD/packages/tool/";
    let info = Installed {
        repository: Some("https://github.com/vendor/repo.git".into()),
        directory: Some("packages/tool".into()),
        ..Default::default()
    };
    let mut fetch = Fake {
        pages: HashMap::from([
            (
                format!("{root}package.json"),
                r#"{"publisher":"vendor","name":"tool","version":"2.0.0"}"#.into(),
            ),
            (format!("{root}CHANGELOG.md"), CHANGELOG.into()),
        ]),
        ..Default::default()
    };
    let r = extensions::notes(
        &fetch,
        "x",
        "vendor.tool",
        "1.0.0",
        &info,
        Mode::Published,
        Channel::Stable,
    )
    .unwrap();
    assert!(r.scope.contains("publication unverified"));
    fetch.pages.insert(
        format!("{root}package.json"),
        r#"{"name":"whole-monorepo","version":"2.0.0"}"#.into(),
    );
    assert!(
        extensions::notes(
            &fetch,
            "x",
            "vendor.tool",
            "1.0.0",
            &info,
            Mode::Published,
            Channel::Stable
        )
        .is_err()
    );
}
#[test]
fn unsafe_assets_and_unmatched_extensions_cannot_supply_notes() {
    let data = query(vec![publication(
        "2.0.0",
        false,
        "universal",
        "https://attacker.example/changelog",
    )]);
    assert!(
        extensions::marketplace_asset(
            &data,
            "vendor.tool",
            "1.0.0",
            &Installed::default(),
            Mode::Published,
            Channel::Stable
        )
        .is_err()
    );
    assert!(
        extensions::notes(
            &Fake::default(),
            "x",
            "../invalid",
            "1.0.0",
            &Installed::default(),
            Mode::Installed,
            Channel::Stable
        )
        .is_err()
    );
}
#[test]
#[ignore = "explicit live Marketplace changelog and HTTP-cache check"]
fn live_marketplace_supabase_changelog() {
    let dir = tempfile::tempdir().unwrap();
    let fetch = HttpClient::new(CacheOptions {
        directory: dir.path().into(),
        ..Default::default()
    })
    .unwrap();
    let r = extensions::notes(
        &fetch,
        "sample",
        "supabase.vscode-supabase-extension",
        "0.0.12",
        &Installed::default(),
        Mode::Published,
        Channel::Stable,
    )
    .unwrap();
    assert!(r.scope.contains("Marketplace"));
    assert!(
        r.notes
            .iter()
            .any(|n| n.version.as_deref() == Some("0.0.13"))
    );
    assert!(!r.complete);
    println!("{}; {} notes", r.scope, r.notes.len());
}
