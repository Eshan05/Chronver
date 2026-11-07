use releasescout_engine::{
    http::{Document, Fetcher},
    watchlist::{self, Ecosystem, Entry, Group},
    *,
};
use serde_json::json;
use std::{cell::RefCell, collections::HashMap};
fn entry(name: &str) -> Entry {
    Entry {
        ecosystem: Ecosystem::Npm,
        name: name.into(),
        group: Group::Core,
    }
}

#[test]
fn personal_subscriptions_persist_without_projects_versions_or_reseeding() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("watchlist.json");
    assert!(watchlist::load(&path).unwrap().is_empty());
    let mut entries = vec![
        entry("next"),
        Entry {
            ecosystem: Ecosystem::Cargo,
            name: "serde".into(),
            group: Group::Tooling,
        },
    ];
    watchlist::save(&path, &entries).unwrap();
    assert_eq!(watchlist::load(&path).unwrap(), entries);
    let id = entries[0].id();
    entries[0].group = Group::Tooling;
    assert_eq!(entries[0].id(), id);
    watchlist::save(&path, &[]).unwrap();
    assert!(watchlist::load(&path).unwrap().is_empty());
    std::fs::write(&path, "broken").unwrap();
    assert!(watchlist::load(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
}

#[test]
fn validation_prevents_duplicates_invalid_paths_and_oversized_lists() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("watchlist.json");
    assert!(watchlist::save(&path, &[entry("next"), entry("next")]).is_err());
    for name in [
        "../next",
        "next@16",
        "https://example.test/pkg",
        "@scope/../pkg",
        "",
    ] {
        assert!(entry(name).validate().is_err());
    }
    assert!(entry("@tanstack/react-table").validate().is_ok());
    let cargo = Entry {
        ecosystem: Ecosystem::Cargo,
        name: "serde_json".into(),
        group: Group::Core,
    };
    assert!(cargo.validate().is_ok());
    assert!(watchlist::save(&path, &vec![entry("next"); 1001]).is_err());
}

#[derive(Default)]
struct Fake {
    pages: HashMap<String, String>,
    calls: RefCell<Vec<String>>,
}
impl Fetcher for Fake {
    fn get(&self, url: &str) -> Result<Document> {
        self.calls.borrow_mut().push(url.into());
        self.pages
            .get(url)
            .map(|body| Document {
                body: body.clone(),
                headers: Default::default(),
                evidence: Evidence {
                    url: url.into(),
                    cache: "fixture".into(),
                    fetched_at: "2026-11-11T00:00:00Z".into(),
                },
            })
            .ok_or_else(|| Error::Source("fixture absent".into()))
    }
}
impl Fetcher for &Fake {
    fn get(&self, url: &str) -> Result<Document> {
        <Fake as Fetcher>::get(self, url)
    }
}
#[test]
fn registry_identity_must_match_and_sparse_notes_stay_honest() {
    let mut fetch = Fake {
        pages: HashMap::from([(
            "https://registry.npmjs.org/fixture-package/latest".into(),
            json!({"name":"fixture-package","version":"2.0.0"}).to_string(),
        )]),
        ..Default::default()
    };
    let r = watchlist::notes(&fetch, &entry("fixture-package"), Channel::Stable).unwrap();
    assert_eq!(r.status, "no_authored_notes");
    assert!(!r.notes[0].authored_notes);
    assert!(r.notes[0].body.is_empty());
    assert_eq!(r.notes[0].version.as_deref(), Some("2.0.0"));
    assert!(r.scope.contains("public release history"));
    assert!(!r.complete);
    assert!(
        fetch
            .calls
            .borrow()
            .iter()
            .all(|u| !u.contains("package.json"))
    );
    fetch.pages.insert(
        "https://registry.npmjs.org/fixture-package/latest".into(),
        json!({"name":"wrong","version":"2.0.0"}).to_string(),
    );
    assert!(watchlist::notes(&fetch, &entry("fixture-package"), Channel::Stable).is_err());
}
#[test]
fn curated_history_and_channels_are_shared_with_project_packages() {
    let fetch=Fake{pages:HashMap::from([
        ("https://registry.npmjs.org/next/latest".into(),json!({"name":"next","version":"16.0.0"}).to_string()),
        ("https://api.github.com/repos/vercel/next.js/releases?per_page=20&page=1".into(),json!([
            {"tag_name":"v17.0.0-canary.1","name":"preview","html_url":"https://github.com/vercel/next.js/releases/tag/v17.0.0-canary.1","body":"Fixed preview rendering.","draft":false,"prerelease":true,"published_at":"2026-11-11T00:00:00Z"},
            {"tag_name":"v16.0.0","name":"stable","html_url":"https://github.com/vercel/next.js/releases/tag/v16.0.0","body":"Fixed production rendering.","draft":false,"prerelease":false,"published_at":"2026-11-11T00:00:00Z"}
        ]).to_string()),
    ]),..Default::default()};
    let r = watchlist::notes(&fetch, &entry("next"), Channel::Stable).unwrap();
    assert_eq!(r.notes.len(), 1);
    assert_eq!(r.notes[0].version.as_deref(), Some("16.0.0"));
    assert!(r.notes[0].authored_notes);
    let r = watchlist::notes(&fetch, &entry("next"), Channel::Preview).unwrap();
    assert!(r.notes.iter().all(|n| n.preview));
}

#[test]
#[ignore = "explicit public registry and release-source check"]
fn live_watchlist_packages() {
    let temp = tempfile::tempdir().unwrap();
    for entry in [
        entry("next"),
        entry("clsx"),
        Entry {
            ecosystem: Ecosystem::Cargo,
            name: "serde_json".into(),
            group: Group::Core,
        },
    ] {
        let fetch = http::HttpClient::new(http::CacheOptions {
            directory: temp.path().into(),
            public_only: true,
            ..Default::default()
        })
        .unwrap();
        let r = watchlist::notes(fetch, &entry, Channel::Stable).unwrap();
        assert!(!r.notes.is_empty(), "{}", entry.name);
        assert!(r.notes.iter().all(|n| !n.preview));
        if entry.name != "serde" {
            assert!(
                r.notes.iter().any(|n| n.authored_notes),
                "{}: {:?}",
                entry.name,
                r.warnings
            );
        }
        println!(
            "{}:{} / {} entries",
            entry.ecosystem.id(),
            entry.name,
            r.notes.len()
        );
    }
}
