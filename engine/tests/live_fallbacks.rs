use releasescout_engine::{
    http::{CacheOptions, Document, Fetcher, HttpClient},
    *,
};
struct NoApi(HttpClient);
impl Fetcher for NoApi {
    fn get(&self, url: &str) -> Result<Document> {
        if url.starts_with("https://api.github.com/") {
            Err(Error::Source(
                "forced API outage for fallback verification".into(),
            ))
        } else {
            self.0.get(url)
        }
    }
    fn npm_dates(&self, package: &str) -> Result<Document> {
        self.0.npm_dates(package)
    }
}
#[test]
#[ignore = "explicit public-network Vercel and Atlas date check"]
fn vercel_and_atlas_publication_dates_use_the_right_sources() {
    let cache = tempfile::tempdir().unwrap();
    let engine = Engine {
        catalog: catalog::bundled().unwrap(),
        fetcher: NoApi(
            HttpClient::new(CacheOptions {
                directory: cache.path().into(),
                public_only: true,
                ..Default::default()
            })
            .unwrap(),
        ),
    };
    let report = engine
        .notes(
            "vercel-cli",
            None,
            Selection::Versions {
                from: "64.5.0".into(),
                to: "65.4.0".into(),
                channel: Channel::Stable,
            },
            false,
        )
        .unwrap();
    let date = |v: &str| {
        report
            .notes
            .iter()
            .find(|n| n.version.as_deref() == Some(v))
            .unwrap_or_else(|| panic!("No note for {v}"))
            .date
            .map(|d| d.to_string())
    };
    assert_eq!(date("65.4.0").as_deref(), Some("2026-10-27"));
    assert_eq!(date("65.3.1").as_deref(), Some("2026-10-26"));
    assert_eq!(date("65.3.0"), None);
    assert!(
        report
            .evidence
            .iter()
            .any(|e| e.url == "https://registry.npmjs.org/vercel")
    );

    let atlas = engine
        .catalog
        .packages
        .iter()
        .find(|p| p.name == "MongoDB Atlas CLI")
        .unwrap();
    let page = engine.history(&atlas.id, None, None, false).unwrap();
    let note = page
        .report
        .notes
        .iter()
        .find(|n| n.version.as_deref() == Some("1.59.1"))
        .unwrap();
    assert_eq!(note.date.unwrap().to_string(), "2026-10-09");
}

#[test]
#[ignore = "explicit public GitHub metadata check"]
fn non_npm_changelogs_get_dates_from_exact_github_releases() {
    let cache = tempfile::tempdir().unwrap();
    let engine = Engine {
        catalog: catalog::bundled().unwrap(),
        fetcher: HttpClient::new(CacheOptions {
            directory: cache.path().into(),
            public_only: true,
            ..Default::default()
        })
        .unwrap(),
    };
    for (id, from, version, expected) in [
        ("cloudflared", "2026.10.7", "2026.11.1", "2026-11-17"),
        ("mise", "2026.11.9", "2026.11.10", "2026-11-18"),
    ] {
        let report = engine
            .notes(
                id,
                None,
                Selection::Versions {
                    from: from.into(),
                    to: version.into(),
                    channel: Channel::Stable,
                },
                false,
            )
            .unwrap();
        let note = report
            .notes
            .iter()
            .find(|n| n.version.as_deref() == Some(version))
            .unwrap_or_else(|| panic!("Missing {id} {version}: {:?}", report.attempts));
        assert_eq!(
            note.date.map(|d| d.to_string()).as_deref(),
            Some(expected),
            "{id}"
        );
        assert!(
            report
                .evidence
                .iter()
                .any(|e| e.url.starts_with("https://api.github.com/"))
        );
    }
}

#[test]
#[ignore = "explicit public-network API-outage and selected-HTML-entry smoke test"]
fn official_feeds_and_scoped_desktop_entries_work_without_github_api() {
    let cache = tempfile::tempdir().unwrap();
    let fetcher = NoApi(
        HttpClient::new(CacheOptions {
            directory: cache.path().into(),
            public_only: true,
            ..Default::default()
        })
        .unwrap(),
    );
    let engine = Engine {
        catalog: catalog::bundled().unwrap(),
        fetcher,
    };
    for id in ["bun", "neovim", "codex", "chatgpt-desktop"] {
        let page = engine.history(id, None, None, false).unwrap();
        assert!(
            page.report.notes.iter().any(|n| n.authored_notes),
            "{id}: {:?}; {:?}",
            page.report.warnings,
            page.report.attempts
        );
        assert!(!page.report.complete);
        println!(
            "{id}: {} notes, {:?}",
            page.report.notes.len(),
            page.report
                .evidence
                .iter()
                .map(|e| &e.url)
                .collect::<Vec<_>>()
        );
    }
}
