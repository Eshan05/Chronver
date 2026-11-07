use releasescout_engine::{
    http::{Document, Fetcher},
    source_discovery::{self, Hints},
    *,
};
use std::{cell::RefCell, collections::HashMap};

#[derive(Default)]
struct Fake {
    pages: HashMap<String, String>,
    calls: RefCell<Vec<String>>,
}
impl Fake {
    fn page(&mut self, url: &str, body: &str) {
        self.pages.insert(url.into(), body.into());
    }
}
impl Fetcher for Fake {
    fn get(&self, url: &str) -> Result<Document> {
        self.calls.borrow_mut().push(url.into());
        Ok(Document {
            body: self
                .pages
                .get(url)
                .ok_or_else(|| Error::Source("fixture miss".into()))?
                .clone(),
            headers: Default::default(),
            evidence: Evidence {
                url: url.into(),
                fetched_at: "2026-11-11T00:00:00Z".into(),
                cache: "fixture".into(),
            },
        })
    }
}
fn hints() -> Hints {
    Hints {
        manager: "scoop".into(),
        source: "main".into(),
        id: "tool".into(),
        installed: Some("1.0.0".into()),
        repository: None,
        directory: None,
        homepage: None,
        release_notes: None,
        downloads: vec![],
        root: None,
        references: vec![],
    }
}
fn release(tag: &str, body: &str) -> String {
    serde_json::json!([{"tag_name":tag,"name":tag,"body":body,"html_url":format!("https://github.com/acme/tool/releases/tag/{tag}"),"prerelease":false,"draft":false,"published_at":null}]).to_string()
}
const API: &str = "https://api.github.com/repos/acme/tool/releases?per_page=20&page=1";
#[test]
fn declared_repository_promotes_authored_notes_but_never_claims_complete() {
    let mut h = hints();
    h.repository = Some("git+https://github.com/acme/tool.git".into());
    let mut f = Fake::default();
    f.page(API, &release("v1.1.0", "Fixed startup crash"));
    let result = source_discovery::discover(&f, &h);
    assert_eq!(result.streams.len(), 1);
    assert!(
        matches!(&result.streams[0].sources[0],Source::Github{owner,repo,..} if owner=="acme" && repo=="tool")
    );
    let p = Package {
        id: "test".into(),
        name: "test".into(),
        tracking: "track".into(),
        identity: "metadata_discovered".into(),
        alias_of: None,
        caveats: String::new(),
        reference_urls: vec![],
        streams: result.streams,
    };
    let page = Engine {
        catalog: Catalog {
            schema_version: 1,
            packages: vec![p],
        },
        fetcher: f,
    }
    .history("test", None, None, false)
    .unwrap();
    assert!(!page.report.complete);
    assert!(
        page.report
            .warnings
            .iter()
            .any(|w| w.contains("Automatically discovered"))
    );
}
#[test]
fn empty_and_sparse_releases_fall_back_to_declared_changelog() {
    for body in ["[]".to_string(), release("v1.1.0", "")] {
        let mut h = hints();
        h.repository = Some("https://github.com/acme/tool".into());
        let mut f = Fake::default();
        f.page(API, &body);
        f.page(
            "https://raw.githubusercontent.com/acme/tool/HEAD/CHANGELOG.md",
            "# Changelog\n\n## 1.1.0\n\nFixed startup crash.",
        );
        let result = source_discovery::discover(&f, &h);
        assert_eq!(result.streams.len(), 1, "{:?}", result.attempts);
        assert!(
            matches!(&result.streams[0].sources[0],Source::Document{url,..} if url.ends_with("CHANGELOG.md"))
        );
    }
}
#[test]
fn component_download_tags_preserve_component_namespace() {
    let mut h = hints();
    h.downloads = vec!["https://github.com/acme/tool/releases/download/cli-v1.0.0/tool.zip".into()];
    let mut f = Fake::default();
    f.page(API, &release("cli-v1.1.0", "Fixed CLI startup"));
    let r = source_discovery::discover(&f, &h);
    assert_eq!(r.streams.len(), 1);
    let Source::Github {
        tag_pattern: Some(pattern),
        ..
    } = &r.streams[0].sources[0]
    else {
        panic!("expected component releases")
    };
    let regex = regex::Regex::new(pattern).unwrap();
    assert!(regex.is_match("cli-v1.1.0"));
    assert!(!regex.is_match("server-v1.1.0"));
}
#[test]
fn scoped_installer_does_not_fall_back_to_repo_wide_notes() {
    let mut h = hints();
    h.repository = Some("https://github.com/acme/tool".into());
    h.downloads = vec!["https://github.com/Acme/Tool/releases/download/cli-v1.0.0/tool.zip".into()];
    let mut f = Fake::default();
    f.page(
        "https://api.github.com/repos/Acme/Tool/releases?per_page=20&page=1",
        &release("v1.1.0", "Notes for the whole suite."),
    );
    f.page(API, &release("v1.1.0", "Notes for the whole suite."));
    f.page(
        "https://raw.githubusercontent.com/acme/tool/HEAD/CHANGELOG.md",
        "## 1.1.0\n\nReleased the whole suite.",
    );
    assert!(source_discovery::discover(&f, &h).streams.is_empty());
    assert_eq!(f.calls.borrow().len(), 1);
    assert!(!f.calls.borrow().iter().any(|u| u.contains("CHANGELOG")));
}

#[test]
fn installer_version_must_match_a_whole_tag_version() {
    for tag in ["cli-v11.0.0", "cli-v1.0.00", "cli-v1.0.0.2", "cli-v2.1.0.0", "cli-v1.0.0-to-1.0.0"] {
        let mut h = hints();
        h.downloads = vec![format!("https://github.com/acme/tool/releases/download/{tag}/tool.zip")];
        let f = Fake::default();
        assert!(source_discovery::discover(&f, &h).streams.is_empty(), "{tag}");
        assert!(f.calls.borrow().is_empty(), "{tag}");
    }
}

#[test]
fn explicit_notes_precede_generic_repo_releases_and_support_github_blobs() {
    for (declared, fetched) in [
        ("https://docs.example.com/product/changes.md", "https://docs.example.com/product/changes.md"),
        ("https://github.com/acme/tool/blob/main/docs/CLI.md", "https://raw.githubusercontent.com/acme/tool/main/docs/CLI.md"),
    ] {
        let mut h = hints();
        h.repository = Some("https://github.com/acme/tool".into());
        h.release_notes = Some(declared.into());
        let mut f = Fake::default();
        f.page(fetched, "## 1.1.0\n\nFixed CLI startup.");
        f.page(API, &release("v1.1.0", "Notes for the whole suite."));
        let found = source_discovery::discover(&f, &h);
        assert!(
            matches!(&found.streams[0].sources[0], Source::Document { url, .. } if url == fetched)
        );
        assert_eq!(f.calls.borrow().as_slice(), [fetched]);
    }
}
#[test]
fn declared_repository_name_prefix_is_allowed_but_unrelated_components_are_not() {
    let mut h = hints();
    h.repository = Some("https://github.com/acme/tool".into());
    let mut f = Fake::default();
    f.page(API, &release("Tool-v1.1.0", "Fixed a startup crash."));
    let r = source_discovery::discover(&f, &h);
    assert!(!r.streams.is_empty());
    f.page(
        API,
        &release("other-v1.1.0", "Unrelated component startup fix."),
    );
    assert!(source_discovery::discover(&f, &h).streams.is_empty());
}
#[test]
fn declared_html_release_documents_can_have_product_titles_or_bold_version_paragraphs() {
    for body in [
        "<h1>Tool 1.1.0 Release Notes</h1><p>Fixed a startup crash.</p>",
        "<p><b>Version 1.1.0</b></p><p>Fixed a startup crash.</p>",
    ] {
        let source = source_discovery::document("https://example.com/changelog", false);
        let notes = parse::document(body, &source).unwrap();
        assert_eq!(notes[0].version.as_deref(), Some("1.1.0"));
        assert!(notes[0].authored_notes);
    }
}
#[test]
fn npm_monorepo_requires_exact_scoped_manifest_and_never_uses_suite_releases() {
    let mut h = hints();
    h.manager = "pnpm".into();
    h.id = "@acme/cli".into();
    h.repository = Some("https://github.com/acme/tool".into());
    let mut f = Fake::default();
    f.page(
        "https://raw.githubusercontent.com/acme/tool/HEAD/package.json",
        r#"{"name":"suite"}"#,
    );
    f.page(API, &release("1.1.0", "Unrelated suite notes"));
    assert!(source_discovery::discover(&f, &h).streams.is_empty());
    assert!(!f.calls.borrow().iter().any(|u| u == API));
    h.directory = Some("packages/cli".into());
    f.page(
        "https://raw.githubusercontent.com/acme/tool/HEAD/packages/cli/package.json",
        r#"{"name":"@acme/cli"}"#,
    );
    f.page(
        "https://raw.githubusercontent.com/acme/tool/HEAD/packages/cli/CHANGELOG.md",
        "## 1.1.0\n\nFixed scoped startup.",
    );
    let r = source_discovery::discover(&f, &h);
    assert_eq!(r.streams.len(), 1, "{:?}", r.attempts);
    assert!(!f.calls.borrow().iter().any(|u| u == API));
}
#[test]
fn website_declared_notes_support_dates_and_reject_unrelated_origins() {
    let mut h = hints();
    h.homepage = Some("https://example.com/".into());
    let mut f = Fake::default();
    f.page("https://example.com/",r#"<a href="https://unrelated.com/changelog">Changelog</a><a href="/release-notes">Release notes</a>"#);
    f.page(
        "https://example.com/release-notes",
        "<h2>November 10, 2026</h2><p>Added a new command.</p>",
    );
    let r = source_discovery::discover(&f, &h);
    assert_eq!(r.streams.len(), 1, "{:?}", r.attempts);
    assert_eq!(r.streams[0].basis, Basis::Date);
    assert!(
        f.calls
            .borrow()
            .iter()
            .all(|u| !u.contains("unrelated.com"))
    );
}
#[test]
fn website_can_declare_an_atom_feed() {
    let mut h = hints();
    h.homepage = Some("https://example.com/".into());
    let mut f = Fake::default();
    f.page(
        "https://example.com/",
        r#"<link rel="alternate" type="application/atom+xml" href="/updates.xml">"#,
    );
    f.page("https://example.com/updates.xml",r#"<feed xmlns="http://www.w3.org/2005/Atom"><id>https://example.com/</id><title>Updates</title><updated>2026-11-10T00:00:00Z</updated><entry><id>https://example.com/one</id><title>Improved search</title><link href="https://example.com/one"/><published>2026-11-10T00:00:00Z</published><updated>2026-11-10T00:00:00Z</updated><content>Fixed a search bug.</content></entry></feed>"#);
    let r = source_discovery::discover(&f, &h);
    assert_eq!(r.streams.len(), 1, "{:?}", r.attempts);
    assert!(matches!(r.streams[0].sources[0], Source::Feed { .. }));
}
#[test]
fn version_heading_on_a_marketing_homepage_is_not_a_changelog() {
    let mut h = hints();
    h.homepage = Some("https://example.com/".into());
    let mut f = Fake::default();
    f.page(
        "https://example.com/",
        "<h1>1.0.0</h1><p>Buy our excellent product.</p>",
    );
    assert!(source_discovery::discover(&f, &h).streams.is_empty());
}
#[test]
fn names_are_not_repository_hints_and_private_urls_are_not_fetched() {
    let mut h = hints();
    let f = Fake::default();
    assert!(!h.has_hints());
    assert!(source_discovery::discover(&f, &h).streams.is_empty());
    assert!(f.calls.borrow().is_empty());
    h.repository = Some("http://127.0.0.1/tool".into());
    h.homepage = Some("https://localhost/changelog".into());
    h.release_notes = Some("https://10.0.0.1/notes".into());
    assert!(source_discovery::discover(&f, &h).streams.is_empty());
    assert!(f.calls.borrow().is_empty());
}
#[test]
fn exact_winget_manifest_provides_notes_but_wrong_identity_is_rejected() {
    let mut h = hints();
    h.manager = "winget".into();
    h.source = "winget".into();
    h.id = "Acme.Tool".into();
    let mut f = Fake::default();
    let url = "https://raw.githubusercontent.com/microsoft/winget-pkgs/master/manifests/a/Acme/Tool/1.0.0/Acme.Tool.locale.en-US.yaml";
    f.page(url,"PackageIdentifier: Acme.Tool\nPackageVersion: 1.0.0\nReleaseNotesUrl: https://example.com/notes.md\n");
    f.page(
        "https://example.com/notes.md",
        "## 1.1.0\n\nFixed startup crash.",
    );
    assert!(h.has_hints());
    assert_eq!(source_discovery::discover(&f, &h).streams.len(), 1);
    f.calls.borrow_mut().clear();
    f.page(url,"PackageIdentifier: Different.Product\nPackageVersion: 1.0.0\nReleaseNotesUrl: https://example.com/notes.md\n");
    assert!(source_discovery::discover(&f, &h).streams.is_empty());
    assert_eq!(f.calls.borrow().len(), 1);
    h.source = "private-registry".into();
    assert!(!h.has_hints());
}
#[test]
fn winget_installer_urls_supply_repository_evidence_without_guessing_package_names() {
    let mut h = hints();
    h.manager = "winget".into();
    h.source = "winget".into();
    h.id = "Acme.Tool".into();
    h.installed = Some("1.0.0.0".into());
    let mut f = Fake::default();
    let root = "https://raw.githubusercontent.com/microsoft/winget-pkgs/master/manifests/a/Acme/Tool/1.0.0.0/";
    f.page(
        &format!("{root}Acme.Tool.locale.en-US.yaml"),
        "PackageIdentifier: Acme.Tool\nPackageVersion: 1.0.0.0\nPackageUrl: https://example.com\n",
    );
    f.page(&format!("{root}Acme.Tool.installer.yaml"),"PackageIdentifier: Acme.Tool\nPackageVersion: 1.0.0.0\nInstallers:\n- InstallerUrl: https://github.com/acme/tool/releases/download/v1.0.0/tool.msi\n");
    f.page(API, &release("v1.1.0", "Fixed a startup crash."));
    let r = source_discovery::discover(&f, &h);
    assert!(!r.streams.is_empty(), "{:?}", r.attempts);
    assert!(
        f.calls.borrow().iter().all(|u| !u.ends_with("tool.msi")),
        "never download installers"
    );
}
#[test]
fn installed_notes_are_version_checked_and_exclude_newer_sections() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("package.json"),
        r#"{"name":"tool","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("CHANGELOG.md"),
        "## 9.0.0\n\nFuture unreleased changes\n\n## 1.0.0\n\nInstalled startup fixes",
    )
    .unwrap();
    let mut h = hints();
    h.root = Some(root.path().display().to_string());
    let f = Fake::default();
    let r = source_discovery::discover(&f, &h);
    assert_eq!(r.streams.len(), 1);
    assert!(matches!(
        r.streams[0].sources[0],
        Source::InstalledDocument { .. }
    ));
    assert!(
        source_discovery::installed_document(
            h.root.as_deref().unwrap(),
            "CHANGELOG.md",
            "tool",
            "2.0.0"
        )
        .is_err()
    );
    assert!(
        source_discovery::installed_document(
            h.root.as_deref().unwrap(),
            "../escape.md",
            "tool",
            "1.0.0"
        )
        .is_err()
    );
    let p = Package {
        id: "test".into(),
        name: "test".into(),
        tracking: "track".into(),
        identity: "metadata_discovered".into(),
        alias_of: None,
        caveats: String::new(),
        reference_urls: vec![],
        streams: r.streams,
    };
    let page = Engine {
        catalog: Catalog {
            schema_version: 1,
            packages: vec![p],
        },
        fetcher: f,
    }
    .history("test", None, None, false)
    .unwrap();
    assert_eq!(page.report.notes.len(), 1);
    assert_eq!(page.report.notes[0].version.as_deref(), Some("1.0.0"));
    assert!(!page.report.complete);
}

#[test]
fn live_declared_repository_opt_in() {
    if std::env::var_os("RELEASESCOUT_LIVE_SOURCE").is_none() {
        return;
    }
    let mut h = hints();
    h.repository = Some("https://github.com/sharkdp/bat".into());
    let dir = tempfile::tempdir().unwrap();
    let r = source_discovery::cached(
        http::CacheOptions {
            directory: dir.path().join("http"),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            ..Default::default()
        },
        &h,
    );
    assert!(!r.streams.is_empty(), "{:?}", r.attempts);
}

#[test]
fn reviewed_raw_reference_and_markup_release_notes_are_reused() {
    let mut h = hints();
    h.references = vec!["https://github.com/acme/tool/blob/main/Changelog".into()];
    let mut f = Fake::default();
    f.page(
        "https://raw.githubusercontent.com/acme/tool/main/Changelog",
        "## 1.0.0\n\nFixed a startup crash.",
    );
    assert_eq!(source_discovery::discover(&f, &h).streams.len(), 1);
    h.references.clear();
    h.release_notes = Some("[Release notes](https://example.com/notes.md)".into());
    f.page(
        "https://example.com/notes.md",
        "## 1.0.0\n\nFixed a startup crash.",
    );
    assert_eq!(source_discovery::discover(&f, &h).streams.len(), 1);
}

#[test]
fn two_part_release_headings_are_not_dropped_for_a_three_part_installed_version() {
    let mut h = hints();
    h.installed = Some("6.1.4".into());
    h.references = vec!["https://example.com/CHANGELOG.md".into()];
    h.release_notes = Some("https://old.example.com/notes".into());
    let mut f = Fake::default();
    f.page(
        "https://example.com/CHANGELOG.md",
        "# 6.1\n\nMove to native filesystem calls.\n# 6.0\n\nDropped old engine compatibility.",
    );
    let found = source_discovery::discover(&f, &h);
    assert_eq!(found.streams.len(), 1);
    assert_eq!(found.streams[0].scheme, Scheme::Numeric);
    assert_eq!(f.calls.borrow()[0], "https://example.com/CHANGELOG.md");
}

#[test]
fn discovery_cache_is_hint_scoped_with_negative_expiry_and_offline_reuse() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let h = hints();
    let sources = dir.path().join("sources");
    std::fs::create_dir(&sources).unwrap();
    let key = format!("{:x}", Sha256::digest(serde_json::to_vec(&h).unwrap()));
    let file = sources.join(format!("{key}.json"));
    let write = |version: u8, age: i64| {
        std::fs::write(&file,serde_json::json!({"version":version,"hints":h,"at":chrono::Utc::now().timestamp()-age,"result":{"streams":[],"attempts":["cached sentinel"]}}).to_string()).unwrap()
    };
    let options = http::CacheOptions {
        directory: dir.path().join("http"),
        ..Default::default()
    };
    write(3, 10);
    assert_ne!(
        source_discovery::cached(options.clone(), &h).attempts,
        vec!["cached sentinel"]
    );
    write(4, 10);
    assert_eq!(
        source_discovery::cached(options.clone(), &h).attempts,
        vec!["cached sentinel"]
    );
    write(4, 4000);
    assert_ne!(
        source_discovery::cached(options.clone(), &h).attempts,
        vec!["cached sentinel"]
    );
    write(4, 4000);
    let offline = http::CacheOptions {
        offline: true,
        ..options.clone()
    };
    assert_eq!(
        source_discovery::cached(offline.clone(), &h).attempts,
        vec!["cached sentinel"]
    );
    let mut changed = h.clone();
    changed.installed = Some("2.0.0".into());
    assert_ne!(
        source_discovery::cached(offline, &changed).attempts,
        vec!["cached sentinel"]
    );
    write(4, 0);
    assert_ne!(
        source_discovery::cached(
            http::CacheOptions {
                refresh: true,
                ..options
            },
            &h
        )
        .attempts,
        vec!["cached sentinel"]
    );
}
