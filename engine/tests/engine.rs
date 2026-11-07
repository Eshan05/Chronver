use releasescout_engine::{
    catalog,
    http::{Document, Fetcher},
    *,
};
use std::{cell::RefCell, collections::HashMap};
#[test]
fn selected_html_entries_do_not_mix_desktop_and_cli_changes() {
    let source = Source::Document {
        url: "https://example.com/changelog".into(),
        format: Format::HtmlEntries,
        selector: Some("li[data-topic=desktop]".into()),
        exclude: None,
        boundary: r"(?P<date>\d{4}-\d{2}-\d{2})".into(),
        date_formats: vec![],
        paragraphs: true,
    };
    let notes=parse::document("<ul><li data-topic='cli'><p>2026-11-11</p><h3>CLI</h3><p>Changed the CLI engine.</p></li><li data-topic='desktop'><p>2026-11-10</p><h3>Desktop</h3><p>Fixed desktop window positioning.</p></li></ul>",&source).unwrap();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].body.contains("window positioning"));
    assert!(!notes[0].body.contains("CLI engine"));
}
#[test]
fn selected_versioned_entries_preserve_nested_disclosures_as_readable_notes() {
    let source = Source::Document {
        url: "https://example.com/changelog".into(),
        format: Format::HtmlEntries,
        selector: Some("li[data-topic=cli]".into()),
        exclude: None,
        boundary: r"Codex CLI (?P<version>\d+\.\d+\.\d+)".into(),
        date_formats: vec![],
        paragraphs: false,
    };
    let notes=parse::document("<ul><li data-topic='cli'><p>2026-11-10</p><h3>Codex CLI 1.1.0</h3><details class='implementation-css'><summary>Details</summary><h2>Fixes</h2><p>Fixed a startup crash.</p></details></li></ul>",&source).unwrap();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].authored_notes);
    assert!(notes[0].body.contains("startup crash"));
    assert!(!notes[0].body.contains("implementation-css"));
}

struct Fake {
    docs: HashMap<String, (String, Option<String>)>,
    calls: RefCell<Vec<String>>,
}
#[test]
fn bundled_desktop_selector_matches_csv_topic_boundaries_not_cli_only_or_similar_topics() {
    let catalog = catalog::bundled().unwrap();
    let source = &catalog
        .packages
        .iter()
        .find(|p| p.id == "chatgpt-desktop")
        .unwrap()
        .streams[0]
        .sources[0];
    let html = r#"<div id="codex-changelog-content"><ul>
        <li data-codex-topics="general,codex-app,codex-mobile"><p>2026-11-09</p><h3>Shared desktop feature</h3><p>A useful desktop feature shipped.</p></li>
        <li data-codex-topics="codex-app,codex-cli"><p>2026-11-08</p><h3>Shared CLI feature</h3><p>A shared feature shipped.</p></li>
        <li data-codex-topics="general,codex-app"><p>2026-11-07</p><h3>Desktop update</h3><p>A desktop fix shipped.</p></li>
        <li data-codex-topics="codex-app"><p>2026-11-06</p><h3>Desktop only</h3><p>Another desktop fix shipped.</p></li>
        <li data-codex-topics="codex-cli"><p>2026-11-05</p><h3>CLI only</h3><p>Do not include CLI-only changes.</p></li>
        <li data-codex-topics="codex-app-other"><p>2026-11-04</p><h3>Unrelated</h3><p>Do not include similar topics.</p></li>
        </ul></div>"#;
    let notes = parse::document(html, source).unwrap();
    assert_eq!(notes.len(), 4);
    assert_eq!(notes[0].date.unwrap().to_string(), "2026-11-09");
    assert!(notes.iter().all(|n| !n.body.contains("Do not include")));
}
impl Fetcher for Fake {
    fn get(&self, url: &str) -> Result<Document> {
        self.calls.borrow_mut().push(url.into());
        let (body, link) = self
            .docs
            .get(url)
            .ok_or_else(|| Error::Source(format!("missing fixture {url}")))?;
        Ok(Document {
            body: body.clone(),
            headers: link
                .as_ref()
                .map(|l| [("link".into(), l.clone())].into())
                .unwrap_or_default(),
            evidence: Evidence {
                url: url.into(),
                fetched_at: "2026-11-10T00:00:00Z".into(),
                cache: "network".into(),
                content_sha256: None,
            },
        })
    }
}
fn document(body: &str) -> Engine<Fake> {
    let json = serde_json::json!({"schema_version":1,"packages":[{"id":"test","name":"Test","tracking":"track","identity":"confirmed","streams":[{"id":"main","scope":"Test CLI","basis":"version","sources":[{"kind":"document","url":"https://example.com/notes","format":"markdown","boundary":r"(?P<version>\d+\.\d+\.\d+(?:-[a-z0-9.]+)?)"}]}]}]});
    Engine {
        catalog: catalog::load(&json.to_string()).unwrap(),
        fetcher: Fake {
            docs: [("https://example.com/notes".into(), (body.into(), None))].into(),
            calls: RefCell::new(vec![]),
        },
    }
}
fn range(from: &str, to: &str) -> Selection {
    Selection::Versions {
        from: from.into(),
        to: to.into(),
        channel: Channel::Stable,
    }
}

#[test]
fn undated_github_changelog_notes_use_exact_published_release_dates() {
    const DOC: &str = "https://raw.githubusercontent.com/example/project/main/CHANGELOG.md";
    const RELEASES: &str =
        "https://api.github.com/repos/example/project/releases?per_page=100&page=1";
    let body = "## 1.3.0 (2026-11-04)\nFixed the desktop startup crash.\n## 1.2.0\nAdded a local export mode.\n## 1.1.0\nImproved a local option.\n## 1.0.0\nInitial working software release.";
    let mut app = document(body);
    let stream = &mut app.catalog.packages[0].streams[0];
    if let Source::Document { url, boundary, .. } = &mut stream.sources[0] {
        *url = DOC.into();
        *boundary = r"(?P<version>\d+\.\d+\.\d+)(?: \((?P<date>\d{4}-\d{2}-\d{2})\))?".into();
    }
    let release = |tag: &str, day: &str, draft: bool| {
        serde_json::json!({
            "tag_name": tag, "published_at": format!("2026-11-{day}T10:00:00Z"),
            "draft": draft, "prerelease": false
        })
    };
    let data = [
        release("v1.3.0", "09", false),
        release("v1.2.0", "03", false),
        release("v1.1.0", "02", false),
        release("1.1.0", "08", false),
        release("v1.0.0", "01", true),
    ];
    app.fetcher.docs.insert(DOC.into(), (body.into(), None));
    app.fetcher.docs.insert(
        RELEASES.into(),
        (serde_json::to_string(&data).unwrap(), None),
    );
    let report = app
        .notes("test", None, range("1.0.0", "1.3.0"), false)
        .unwrap();
    assert_eq!(
        report
            .notes
            .iter()
            .map(|n| n.date.map(|d| d.to_string()))
            .collect::<Vec<_>>(),
        [Some("2026-11-04".into()), Some("2026-11-03".into()), None]
    );
    assert!(report.notes[1].body.contains("local export"));
    assert!(report.notes[1].url.starts_with(DOC));
    assert_eq!(report.evidence.last().unwrap().url, RELEASES);
    assert!(report.evidence[0].content_sha256.is_some());
    assert_eq!(app.fetcher.calls.borrow().as_slice(), [DOC, RELEASES]);

    let history = app.history("test", None, None, false).unwrap();
    assert_eq!(history.report.notes[1].date, report.notes[1].date);
    assert!(history.report.notes.last().unwrap().date.is_none());
    app.fetcher.docs.remove(RELEASES);
    let offline = app
        .notes("test", None, range("1.0.0", "1.3.0"), false)
        .unwrap();
    assert_eq!(offline.notes[1].body, report.notes[1].body);
    assert!(offline.notes[1].date.is_none());

    const SCOPED: &str =
        "https://raw.githubusercontent.com/example/project/main/packages/tool/CHANGELOG.md";
    let stream = &mut app.catalog.packages[0].streams[0];
    if let Source::Document { url, .. } = &mut stream.sources[0] {
        *url = SCOPED.into();
    }
    stream.sources.push(Source::Github {
        owner: "example".into(),
        repo: "project".into(),
        tag_prefix: "tool-".into(),
        tag_pattern: None,
        max_pages: 1,
    });
    app.fetcher.docs.insert(SCOPED.into(), (body.into(), None));
    let scoped_releases = [
        release("v1.2.0", "06", false),
        release("tool-v1.2.0", "03", false),
    ];
    app.fetcher.docs.insert(
        RELEASES.into(),
        (serde_json::to_string(&scoped_releases).unwrap(), None),
    );
    let scoped = app.history("test", None, None, false).unwrap();
    assert_eq!(scoped.report.notes[1].date, report.notes[1].date);
}
#[test]
fn npm_publication_fallback_keeps_authored_notes_and_does_not_guess_dates() {
    const DOC: &str = "https://raw.githubusercontent.com/example/project/main/packages/cli/CHANGELOG.md";
    const REGISTRY: &str = "https://registry.npmjs.org/example-cli";
    const BODY: &str = "## 1.4.0 (2026-11-06)\nFixed a Windows startup crash.\n## 1.3.0\nAdded a local export mode.\n## 1.2.0\nImproved credential storage.\n## 1.1.0\nImproved local debugging.\n## 1.0.0\nInitial working software release.";
    let mut app = document(BODY);
    let stream = &mut app.catalog.packages[0].streams[0];
    stream.npm_date_package = Some("example-cli".into());
    if let Source::Document { url, boundary, .. } = &mut stream.sources[0] {
        *url = DOC.into();
        *boundary = r"(?P<version>\d+\.\d+\.\d+)(?: \((?P<date>\d{4}-\d{2}-\d{2})\))?".into();
    }
    app.fetcher.docs.insert(DOC.into(), (BODY.into(), None));
    let mut registry = serde_json::json!({
        "name": "example-cli",
        "versions": { "1.4.0": {}, "1.3.0": {}, "1.2.1": {} },
        "time": {
            "1.4.0": "2026-11-07T09:00:00Z",
            "1.3.0": "2026-11-04T01:00:00+02:00",
            "1.2.1": "2026-11-02T09:00:00Z",
            "1.1.0": "2026-11-01T09:00:00Z"
        }
    });
    app.fetcher.docs.insert(REGISTRY.into(), (registry.to_string(), None));
    let report = app.notes("test", None, range("1.0.0", "1.4.0"), false).unwrap();
    assert_eq!(report.notes.iter().map(|n| n.date.map(|d| d.to_string())).collect::<Vec<_>>(),
        [Some("2026-11-06".into()), Some("2026-11-03".into()), None, None]);
    assert_eq!(report.notes[1].body, "Added a local export mode.");
    assert!(report.notes[1].url.starts_with(DOC));
    assert_eq!(report.evidence.last().unwrap().url, REGISTRY);
    assert_eq!(app.fetcher.calls.borrow().as_slice(), [DOC, REGISTRY]);
    let history = app.history("test", None, None, false).unwrap();
    assert_eq!(history.report.notes[1].date, report.notes[1].date);
    app.fetcher.calls.borrow_mut().clear();
    let version_only = app.version_snapshot("test", "main").unwrap();
    assert!(version_only.notes[1].date.is_none());
    assert_eq!(app.fetcher.calls.borrow().as_slice(), [DOC]);

    registry["name"] = "wrong-package".into();
    app.fetcher.docs.insert(REGISTRY.into(), (registry.to_string(), None));
    let report = app.notes("test", None, range("1.0.0", "1.4.0"), false).unwrap();
    assert!(report.notes[1].date.is_none());
    assert_eq!(report.evidence.len(), 1);
}

#[test]
fn atlas_release_date_comes_from_the_publisher_changelog() {
    let catalog = catalog::bundled().unwrap();
    let package = catalog.packages.iter().find(|p| p.name == "MongoDB Atlas CLI").unwrap();
    let source = &package.streams[0].sources[0];
    let notes = parse::document(
        "<main><h2>Atlas CLI 1.59.1</h2><p>Released 09 October 2026</p><p>Added platform support.</p></main>",
        source,
    ).unwrap();
    assert_eq!(notes[0].date.unwrap().to_string(), "2026-10-09");
}

#[test]
fn selects_entire_interval_by_version_not_source_order() {
    let e = document(
        "## 1.3.0\nFixed third real bug\n## 1.1.0\nFixed first real bug\n## 1.2.0\nFixed second real bug\n## 1.0.0\nInitial working software release",
    );
    let r = e
        .notes("test", None, range("1.0.0", "1.3.0"), false)
        .unwrap();
    assert!(r.complete);
    assert_eq!(
        r.notes
            .iter()
            .map(|n| n.version.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["1.3.0", "1.2.0", "1.1.0"]
    );
}
#[test]
fn boundaries_sparse_bodies_and_channels_are_explicit() {
    let e = document(
        "## 1.2.0\n**Full Changelog**: https://github.com/a/b/compare/v1...v2\n## 1.2.0-beta.1\nFixed a preview bug\n## 1.0.0\nInitial working software release",
    );
    let r = e
        .notes("test", None, range("1.0.0", "1.2.0"), false)
        .unwrap();
    assert!(!r.complete);
    assert_eq!(r.status, "no_authored_notes");
    assert_eq!(r.notes.len(), 1);
    let r = e
        .notes(
            "test",
            None,
            Selection::Versions {
                from: "1.0.0".into(),
                to: "1.2.0".into(),
                channel: Channel::Preview,
            },
            false,
        )
        .unwrap();
    assert_eq!(r.notes[0].version.as_deref(), Some("1.2.0-beta.1"));
    let r = e
        .notes("test", None, range("1.0.0", "1.3.0"), false)
        .unwrap();
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("target version boundary"))
    );
    assert!(
        e.notes("test", None, range("1.3.0", "1.0.0"), false)
            .is_err()
    );
}
#[test]
fn github_follows_safe_pagination_and_keeps_out_of_order_releases() {
    let mut e = document("");
    e.catalog.packages[0].streams[0].sources = vec![Source::Github {
        owner: "a".into(),
        repo: "b".into(),
        tag_prefix: "pkg@".into(),
        tag_pattern: None,
        max_pages: 2,
    }];
    let release = |v: &str, date: &str| serde_json::json!({"tag_name":format!("pkg@{v}"),"name":v,"body":"Fixed a real bug", "html_url":format!("https://github.com/a/b/releases/tag/pkg@{v}"),"published_at":date,"draft":false,"prerelease":false});
    let first = "https://api.github.com/repos/a/b/releases?per_page=100&page=1";
    let second = "https://api.github.com/repos/a/b/releases?per_page=100&page=2";
    e.fetcher.docs = [
        (
            first.into(),
            (
                serde_json::json!([
                    release("1.0.0", "2026-11-09T00:00:00Z"),
                    release("1.2.0", "2026-11-08T00:00:00Z")
                ])
                .to_string(),
                Some("<https://api.github.com/repositories/123/releases?per_page=100&page=2>; rel=\"next\"".into()),
            ),
        ),
        (
            second.into(),
            (
                serde_json::json!([release("1.1.0", "2026-11-10T00:00:00Z")]).to_string(),
                None,
            ),
        ),
    ]
    .into();
    let r = e
        .notes("test", None, range("1.0.0", "1.2.0"), false)
        .unwrap();
    assert!(r.complete);
    assert_eq!(r.notes.len(), 2);
    assert_eq!(e.fetcher.calls.borrow().len(), 2);
    e.fetcher.docs.get_mut(first).unwrap().1 =
        Some("<https://evil.example/steal>; rel=\"next\"".into());
    assert_eq!(
        e.notes("test", None, range("1.0.0", "1.2.0"), false)
            .unwrap()
            .status,
        "partial"
    );
}
#[test]
fn cap_is_partial_and_failed_primary_can_fall_back() {
    let mut e = document("## 1.1.0\nFixed a real bug\n## 1.0.0\nInitial working software release");
    e.catalog.packages[0].streams[0].sources.insert(
        0,
        Source::Github {
            owner: "missing".into(),
            repo: "missing".into(),
            tag_prefix: "".into(),
            tag_pattern: None,
            max_pages: 1,
        },
    );
    let r = e
        .notes("test", None, range("1.0.0", "1.1.0"), false)
        .unwrap();
    assert!(r.complete);
    assert_eq!(r.attempts.len(), 2);
    assert_eq!(r.attempts[0].outcome, "failed");
    let first = "https://api.github.com/repos/missing/missing/releases?per_page=100&page=1";
    e.fetcher.docs.insert(first.into(),(r#"[{"tag_name":"1.1.0","name":null,"body":"Fixed a real bug","html_url":"https://github.com/a/b","published_at":null,"draft":false,"prerelease":false}]"#.into(),Some("<https://api.github.com/repos/missing/missing/releases?per_page=100&page=2>; rel=\"next\"".into())));
    e.catalog.packages[0].streams[0].sources.truncate(1);
    let r = e
        .notes("test", None, range("1.0.0", "1.1.0"), false)
        .unwrap();
    assert!(!r.complete);
    assert!(r.warnings.iter().any(|w| w.contains("pagination cap")));
}
#[test]
fn ambiguous_versions_do_not_claim_completeness() {
    let e = document(
        "## 1.1.0\nFirst variation of notes\n## 1.1.0\nDifferent variation of notes\n## 1.0.0\nInitial working software release",
    );
    let r = e
        .notes("test", None, range("1.0.0", "1.1.0"), false)
        .unwrap();
    assert!(!r.complete);
    assert!(r.warnings.iter().any(|w| w.contains("duplicate version")));
}
#[test]
fn unrelated_ambiguities_do_not_poison_a_version_interval() {
    let e = document("## 2.0.0\nFirst future variation.\n## 2.0.0\nSecond future variation.\n## 1.1.0\nFixed a real regression.\n## 1.0.0\nInitial working release.\n## 0.5.0\nFirst historical variation.\n## 0.5.0\nSecond historical variation.");
    let r = e.notes("test", None, range("1.0.0", "1.1.0"), false).unwrap();
    assert!(r.complete);
    assert!(!r.warnings.iter().any(|w| w.contains("duplicate version")));
    let r = e.notes("test", None, Selection::Inspect, false).unwrap();
    assert!(!r.complete);
    assert!(r.warnings.iter().any(|w| w.contains("duplicate version")));
}
#[test]
fn ambiguous_installed_boundary_is_still_partial() {
    let e = document("## 1.1.0\nFixed a real regression.\n## 1.0.0\nFirst baseline variation.\n## 1.0.0\nSecond baseline variation.");
    let r = e.notes("test", None, range("1.0.0", "1.1.0"), false).unwrap();
    assert!(!r.complete);
    assert!(r.warnings.iter().any(|w| w.contains("duplicate version")));
}
#[test]
fn duplicate_in_an_excluded_channel_does_not_poison_stable_bounds() {
    let mut e = document("");
    e.catalog.packages[0].streams[0].sources = vec![Source::Github {
        owner: "a".into(), repo: "b".into(), tag_prefix: String::new(), tag_pattern: None, max_pages: 1,
    }];
    let release = |v: &str, preview: bool| serde_json::json!({"tag_name":v,"name":null,"body":"Fixed a real regression.","html_url":format!("https://github.com/a/b/releases/{v}"),"published_at":null,"draft":false,"prerelease":preview});
    e.fetcher.docs = [("https://api.github.com/repos/a/b/releases?per_page=100&page=1".into(), (
        serde_json::json!([release("1.1.0", true), release("1.1.0", false), release("1.0.0", false)]).to_string(), None
    ))].into();
    let r = e.notes("test", None, range("1.0.0", "1.1.0"), false).unwrap();
    assert!(r.complete);
    assert_eq!(r.notes.len(), 1);
    assert!(!r.notes[0].preview);
    let r = e.notes("test", None, Selection::Inspect, false).unwrap();
    assert!(!r.complete);
    assert!(r.warnings.iter().any(|w| w.contains("duplicate version")));
}
#[test]
fn component_selection_and_inactive_preferences_are_enforced() {
    let mut e = document("## 1.0.0\nInitial working software release");
    let mut s = e.catalog.packages[0].streams[0].clone();
    s.id = "hosted".into();
    s.basis = Basis::Date;
    e.catalog.packages[0].streams.push(s);
    assert!(e.notes("test", None, Selection::Inspect, false).is_err());
    assert!(
        e.notes("test", Some("hosted"), range("1.0.0", "1.1.0"), false)
            .is_err()
    );
    e.catalog.packages[0].tracking = "excluded".into();
    assert!(
        e.notes("test", Some("main"), Selection::Inspect, false)
            .is_err()
    );
    assert!(
        e.notes("test", Some("main"), Selection::Inspect, true)
            .is_ok()
    );
}
#[test]
fn dated_updates_keep_same_day_posts_and_check_history() {
    let mut e = document("");
    let s = &mut e.catalog.packages[0].streams[0];
    s.basis = Basis::Date;
    s.sources = vec![Source::Document {
        url: "https://example.com/notes".into(),
        format: Format::Markdown,
        boundary: r"(?P<date>\d{4}-\d{2}-\d{2})".into(),
        selector: None,
        exclude: None,
        date_formats: vec![],
        paragraphs: false,
    }];
    e.fetcher.docs.get_mut("https://example.com/notes").unwrap().0 = "## 2026-11-09\nFirst feature shipped today\n## 2026-11-09\nSecond feature shipped today\n## 2026-11-01\nPrevious feature shipped here".into();
    let r = e
        .notes(
            "test",
            None,
            Selection::Dates {
                since: "2026-11-01".parse().unwrap(),
                until: "2026-11-10".parse().unwrap(),
            },
            false,
        )
        .unwrap();
    assert!(r.complete);
    assert_eq!(r.notes.len(), 2);
}
#[test]
fn linked_documents_are_range_bounded_and_same_origin() {
    let mut e = document("");
    e.catalog.packages[0].streams[0].sources = vec![Source::LinkedDocuments {
        url: "https://example.com/notes".into(),
        links: "a[href]".into(),
        version_pattern: r"/(?P<version>\d+\.\d+\.\d+)/$".into(),
        content: "main".into(),
        boundary: None,
        allowed_origins: vec![],
        max_documents: 3,
    }];
    e.fetcher.docs = [("https://example.com/notes".into(),("<a href='/1.0.0/'>old</a><a href='/1.1.0/'>new</a><a href='/1.2.0/'>future</a><a href='https://evil.example/1.1.0/'>external</a>".into(),None)),
        ("https://example.com/1.0.0/".into(),("<main>Initial working software release</main>".into(),None)),
        ("https://example.com/1.1.0/".into(),("<main>Fixed a real bug</main>".into(),None))].into();
    let r = e
        .notes("test", None, range("1.0.0", "1.1.0"), false)
        .unwrap();
    assert!(r.complete);
    assert_eq!(r.notes.len(), 1);
    assert_eq!(e.fetcher.calls.borrow().len(), 3);
}
#[test]
fn redirected_indexes_and_articles_resolve_links_at_their_canonical_urls() {
    struct Redirected;
    impl Fetcher for Redirected {
        fn get(&self, url: &str) -> Result<Document> {
            let (body, canonical) = match url {
                "https://example.com/notes" => (
                    "<a href='1.1.0/'>Release</a>",
                    "https://new.example/releases/",
                ),
                "https://new.example/releases/1.1.0/" => (
                    "<main>Fixed a real bug. <a href='details'>Details</a></main>",
                    "https://new.example/archive/1.1.0/",
                ),
                _ => return Err(Error::Source(format!("unexpected URL {url}"))),
            };
            Ok(Document {
                body: body.into(),
                headers: Default::default(),
                evidence: Evidence {
                    url: canonical.into(),
                    fetched_at: "2026-11-10T00:00:00Z".into(),
                    cache: "network".into(),
                    content_sha256: None,
                },
            })
        }
    }
    let mut catalog = document("").catalog;
    catalog.packages[0].streams[0].sources = vec![Source::LinkedDocuments {
        url: "https://example.com/notes".into(),
        links: "a[href]".into(),
        version_pattern: r"/(?P<version>\d+\.\d+\.\d+)/$".into(),
        content: "main".into(),
        boundary: None,
        allowed_origins: vec![],
        max_documents: 3,
    }];
    let engine = Engine {
        catalog,
        fetcher: Redirected,
    };
    let report = engine
        .notes("test", None, Selection::Inspect, false)
        .unwrap();
    assert_eq!(report.notes.len(), 1);
    assert_eq!(report.notes[0].url, "https://new.example/archive/1.1.0/");
    assert!(
        report.notes[0]
            .body
            .contains("https://new.example/archive/1.1.0/details")
    );
}

#[test]
fn release_line_indexes_keep_patch_notes_and_earlier_pages_when_a_later_page_fails() {
    let mut engine = document("");
    engine.catalog.packages[0].streams[0].sources = vec![Source::LinkedDocuments {
        url: "https://example.com/notes".into(),
        links: "a[href]".into(),
        version_pattern: r"/releases/(?P<version>\d+\.\d+)$".into(),
        content: "article".into(),
        boundary: Some(r"(?P<version>\d+\.\d+\.\d+)".into()),
        allowed_origins: vec![],
        max_documents: 3,
    }];
    engine.fetcher.docs=[
        ("https://example.com/notes".into(),("<a href='/releases/4.0'>4.0</a><a href='/releases/3.1'>3.1</a><a href='/releases/3.2'>3.2</a><a href='https://evil.example/releases/99.0'>External</a>".into(),None)),
        ("https://example.com/releases/4.0".into(),("<article><h2>4.0.1</h2><h2>Fixes</h2><p>Fixed a Windows startup crash.</p></article>".into(),None)),
        ("https://example.com/releases/3.1".into(),("<article><h2>3.1.5</h2><p>Fixed a Windows shutdown crash.</p></article>".into(),None)),
    ].into();
    let page = engine.history("test", None, None, false).unwrap();
    assert_eq!(page.report.notes.len(), 2);
    assert!(!page.report.complete);
    assert!(
        page.report
            .warnings
            .iter()
            .any(|w| w.contains("later linked document failed"))
    );
    assert!(
        !engine
            .fetcher
            .calls
            .borrow()
            .iter()
            .any(|u| u.contains("evil.example"))
    );
}
#[test]
fn tag_patterns_filter_other_products_and_later_failures_retain_notes() {
    let mut e = document("");
    e.catalog.packages[0].streams[0].sources = vec![Source::Github {
        owner: "a".into(),
        repo: "b".into(),
        tag_prefix: String::new(),
        tag_pattern: Some(r"(?:pkg@|v)(?P<version>\d+\.\d+\.\d+)".into()),
        max_pages: 2,
    }];
    let first = "https://api.github.com/repos/a/b/releases?per_page=100&page=1";
    let release = |tag: &str, preview: bool| serde_json::json!({"tag_name":tag,"name":null,"body":"Fixed a real bug","html_url":format!("https://github.com/a/b/releases/tag/{tag}"),"published_at":null,"draft":false,"prerelease":preview});
    e.fetcher.docs = [(first.into(),(serde_json::json!([release("pkg@1.0.0",false),release("v1.1.0",false),release("v1.2.0",true),release("other@99.0.0",false)]).to_string(),
        Some("<https://api.github.com/repositories/12/releases?page=2&per_page=100>; rel=\"next\"".into())))].into();
    let r = e
        .notes("test", None, range("1.0.0", "1.2.0"), false)
        .unwrap();
    assert!(!r.complete);
    assert_eq!(r.notes.len(), 1);
    assert_eq!(r.notes[0].version.as_deref(), Some("1.1.0"));
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("later GitHub page failed"))
    );
    assert!(!r.warnings.iter().any(|w| w.contains("99.0.0")));
}
#[test]
fn github_release_bodies_preserve_authored_markdown() {
    let mut e = document("");
    e.catalog.packages[0].streams[0].sources = vec![Source::Github {
        owner: "fixture-org".into(),
        repo: "fixture-tool".into(),
        tag_prefix: "v".into(),
        tag_pattern: None,
        max_pages: 1,
    }];
    e.fetcher.docs = [(
        "https://api.github.com/repos/fixture-org/fixture-tool/releases?per_page=100&page=1".into(),
        (include_str!("fixtures/github-releases.json").into(), None),
    )]
    .into();
    let r = e
        .notes("test", None, range("1.2.0", "1.2.1"), false)
        .unwrap();
    assert!(r.complete);
    assert_eq!(r.notes.len(), 1);
    assert_eq!(
        r.notes[0].body.replace("\r\n", "\n"),
        "## Changes\n\n- Preserve the selected profile when opening a second workspace.\n- Add `--fixture-profile` for isolated runs.\n\nSee [fixture details](https://example.invalid/fixture/details)."
    );
    assert_eq!(
        r.notes[0].url,
        "https://github.com/fixture-org/fixture-tool/releases/tag/v1.2.1"
    );
}

#[test]
fn same_level_subsections_are_preserved_and_parent_ambiguity_is_rejected() {
    let e = document(
        "## 1.1.0\nRelease overview is here.\n## Fixes\nFixed important Windows crash.\n## 1.0.0\nInitial release is here.",
    );
    let r = e
        .notes("test", None, range("1.0.0", "1.1.0"), false)
        .unwrap();
    assert!(r.complete);
    assert!(r.notes[0].body.contains("Fixed important Windows crash"));
    let e = document(
        "## 1.1.0\nRelease overview is here.\n# Appendix\nUnrelated content is here.\n## 1.0.0\nInitial release is here.",
    );
    assert_eq!(
        e.notes("test", None, Selection::Inspect, false)
            .unwrap()
            .status,
        "source_failed"
    );
}

#[test]
fn empty_complete_fallback_does_not_erase_partial_posts() {
    let mut e = document("");
    let s = &mut e.catalog.packages[0].streams[0];
    s.basis = Basis::Date;
    let source = |url: &str| Source::Document {
        url: url.into(),
        format: Format::Markdown,
        boundary: r"(?P<date>\d{4}-\d{2}-\d{2})".into(),
        selector: None,
        exclude: None,
        date_formats: vec![],
        paragraphs: false,
    };
    s.sources = vec![
        source("https://example.com/primary"),
        source("https://example.com/fallback"),
    ];
    e.fetcher.docs = [
        (
            "https://example.com/primary".into(),
            ("## 2026-11-09\nImportant change shipped here.".into(), None),
        ),
        (
            "https://example.com/fallback".into(),
            ("## 2026-11-01\nPrevious changes shipped here.".into(), None),
        ),
    ]
    .into();
    let r = e
        .notes(
            "test",
            None,
            Selection::Dates {
                since: "2026-11-01".parse().unwrap(),
                until: "2026-11-10".parse().unwrap(),
            },
            false,
        )
        .unwrap();
    assert!(!r.complete);
    assert_eq!(r.notes.len(), 1);
    assert_eq!(r.attempts.len(), 2);
}

#[test]
fn mismatched_channel_boundaries_are_partial() {
    let e = document(
        "## 1.1.0-beta.1\nImportant preview changes here.\n## 1.0.0\nPrevious stable changes here.",
    );
    let r = e
        .notes("test", None, range("1.0.0", "1.1.0-beta.1"), false)
        .unwrap();
    assert!(!r.complete);
    assert!(r.notes.is_empty());
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("target version boundary excluded"))
    );
    let mut e = document("");
    e.catalog.packages[0].streams[0].sources = vec![Source::Github {
        owner: "a".into(),
        repo: "b".into(),
        tag_prefix: String::new(),
        tag_pattern: None,
        max_pages: 1,
    }];
    let release = |v: &str, preview: bool| serde_json::json!({"tag_name":v,"name":null,"body":"Fixed a real bug","html_url":"https://github.com/a/b","published_at":null,"draft":false,"prerelease":preview});
    e.fetcher.docs = [(
        "https://api.github.com/repos/a/b/releases?per_page=100&page=1".into(),
        (
            serde_json::json!([release("1.0.0", false), release("1.1.0", true)]).to_string(),
            None,
        ),
    )]
    .into();
    let r = e
        .notes("test", None, range("1.0.0", "1.1.0"), false)
        .unwrap();
    assert!(!r.complete);
    assert!(r.notes.is_empty());
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("target version boundary excluded"))
    );
}

#[test]
fn feed_edits_are_not_new_publications() {
    let mut e = document("");
    e.catalog.packages[0].streams[0].basis = Basis::Date;
    e.catalog.packages[0].streams[0].sources = vec![Source::Feed {
        url: "https://example.com/feed".into(),
    }];
    e.fetcher.docs = [("https://example.com/feed".into(),(r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Updates</title><id>urn:feed</id><updated>2026-11-09T00:00:00Z</updated><entry><id>urn:old</id><title>Old post edited</title><updated>2026-11-09T00:00:00Z</updated><summary>Important content still retained.</summary></entry></feed>"#.into(),None))].into();
    let r = e.notes("test", None, Selection::Inspect, false).unwrap();
    assert!(!r.complete);
    assert_eq!(r.notes.len(), 1);
    assert!(r.notes[0].date.is_none());
    let r = e
        .notes(
            "test",
            None,
            Selection::Dates {
                since: "2026-11-01".parse().unwrap(),
                until: "2026-11-10".parse().unwrap(),
            },
            false,
        )
        .unwrap();
    assert!(!r.complete);
    assert!(r.notes.is_empty());
}

#[test]
fn malformed_later_metadata_keeps_earlier_notes_and_evidence() {
    for link in [
        Some("broken; rel=\"next\""),
        Some("<https://evil.example/steal>; rel=\"next\""),
        None,
    ] {
        let mut e = document("");
        e.catalog.packages[0].streams[0].sources = vec![Source::Github {
            owner: "a".into(),
            repo: "b".into(),
            tag_prefix: String::new(),
            tag_pattern: None,
            max_pages: 3,
        }];
        let release = |v: &str, date: &str| serde_json::json!({"tag_name":v,"name":null,"body":"Fixed a real bug","html_url":"https://github.com/a/b","published_at":date,"draft":false,"prerelease":false});
        let first = "https://api.github.com/repos/a/b/releases?per_page=100&page=1";
        let second = "https://api.github.com/repos/a/b/releases?per_page=100&page=2";
        e.fetcher.docs = [
            (
                first.into(),
                (
                    serde_json::json!([
                        release("1.0.0", "2026-11-01T00:00:00Z"),
                        release("1.1.0", "2026-11-02T00:00:00Z")
                    ])
                    .to_string(),
                    Some(format!("<{second}>; rel=\"next\"")),
                ),
            ),
            (
                second.into(),
                (
                    serde_json::json!([release("1.2.0", "invalid date")]).to_string(),
                    link.map(str::to_string),
                ),
            ),
        ]
        .into();
        let r = e
            .notes("test", None, range("1.0.0", "1.2.0"), false)
            .unwrap();
        assert!(!r.complete);
        assert_eq!(r.notes.len(), 2);
        assert_eq!(r.evidence.len(), 2);
        assert_eq!(e.fetcher.calls.borrow().len(), 2);
        assert!(
            r.warnings
                .iter()
                .any(|w| w.contains("invalid publication date"))
        );
        if link.is_some() {
            assert!(r.warnings.iter().any(|w| w.contains("pagination stopped")));
        }
    }
}

#[test]
fn scope_caveats_survive_success_and_source_failure() {
    let mut e = document("## 1.0.0\nInitial release is here.");
    e.catalog.packages[0].caveats = "Nightly channel only.".into();
    e.catalog.packages[0].streams[0].caveats = "Provisional version mapping.".into();
    for fail in [false, true] {
        if fail {
            e.fetcher.docs.clear();
        }
        let r = e.notes("test", None, Selection::Inspect, false).unwrap();
        assert!(r.warnings.iter().any(|w| w == "Nightly channel only."));
        assert!(
            r.warnings
                .iter()
                .any(|w| w == "Provisional version mapping.")
        );
    }
}
