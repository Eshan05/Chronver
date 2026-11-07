use releasescout_engine::{
    catalog,
    http::{CacheOptions, HttpClient},
    parse, *,
};

struct Borrowed<'a>(&'a HttpClient);
impl http::Fetcher for Borrowed<'_> {
    fn get(&self, url: &str) -> Result<http::Document> {
        http::Fetcher::get(self.0, url)
    }
}

fn document(format: Format, selector: Option<&str>, boundary: &str) -> Source {
    Source::Document {
        url: "https://example.com/notes".into(),
        format,
        selector: selector.map(Into::into),
        exclude: None,
        boundary: boundary.into(),
        date_formats: vec![],
        paragraphs: false,
    }
}

#[test]
#[ignore = "explicit cached Google Cloud document verification; set RELEASESCOUT_GCLOUD_CACHE"]
fn cached_gcloud_release_document() {
    let path = std::path::PathBuf::from(std::env::var_os("RELEASESCOUT_GCLOUD_CACHE").unwrap());
    let file = std::fs::File::open(&path).unwrap();
    let cache: serde_json::Value = if path.extension().is_some_and(|ext| ext == "gz") {
        serde_json::from_reader(flate2::read::GzDecoder::new(std::io::BufReader::new(file)))
    } else {
        serde_json::from_reader(std::io::BufReader::new(file))
    }
    .unwrap();
    let body = cache["body"].as_str().unwrap();
    let catalog = catalog::bundled().unwrap();
    let source = &catalog.packages.iter().find(|p|p.id=="gcloud").unwrap().streams[0].sources[0];
    let started = std::time::Instant::now();
    let notes = parse::document(body, source).unwrap();
    eprintln!("{:?}, {} releases, first page {} bytes; first {:?}", started.elapsed(), notes.len(), notes.iter().take(20).map(|n|n.body.len()).sum::<usize>(), notes.iter().take(3).map(|n|(&n.title, n.body.len())).collect::<Vec<_>>());
    assert!(notes.len()>100);
    assert!(notes.iter().all(|n|n.version.is_some() && n.date.is_some()));
    assert!(notes.iter().all(|n|n.body.len()<100_000));
}

#[test]
fn gcloud_sections_use_versions_and_dates_not_body_dates_or_embedded_notices() {
    let catalog = catalog::bundled().unwrap();
    let profile = catalog.packages.iter().find(|p|p.id=="gcloud").unwrap();
    let source = &profile.streams[0].sources[0];
    let html = r#"<nav>Site navigation</nav><div class="devsite-article-body">
        <h1>Google Cloud CLI - Release Notes</h1>
        <h2>593.0.0 (2026-11-10)</h2><h3>Google Cloud CLI</h3>
        <ul><li>Made bundled Python a required component on Windows.</li></ul>
        <h2>592.0.0 (2026-11-03)</h2><p>2026-08-20</p>
        <ul><li># Release notes for developer_knowledge component.</li></ul>
        <p>Fixed command configuration.</p>
        <h2>591.0.0 (2026-10-27)</h2><p>Previous release changes.</p>
        </div><footer>Last updated 2026-11-10 UTC.</footer>"#;
    let notes = parse::document(html, source).unwrap();
    assert_eq!(notes.len(),3);
    assert_eq!(notes[0].version.as_deref(),Some("593.0.0"));
    assert_eq!(notes[0].date.unwrap().to_string(),"2026-11-10");
    assert!(notes[0].body.contains("bundled Python"));
    assert!(!notes[0].body.contains("Fixed command configuration"));
    assert!(notes[1].body.contains("Release notes for developer"));
    assert!(notes[1].body.contains("Fixed command configuration"));
    assert_eq!(notes[1].date.unwrap().to_string(),"2026-11-03");
    assert!(!notes[2].body.contains("Last updated"));
}

#[test]
fn nested_headings_stay_in_the_release_but_real_enclosing_headings_are_rejected() {
    let source = document(Format::Markdown,None,r"(?P<version>\d+\.\d+\.\d+)");
    for nested in ["- # Component notice\n\n  Fixed component configuration.",
        "> # Compatibility\n> Keep the previous configuration."] {
        let text = format!("## 1.2.0\n\nFixed startup.\n\n{nested}\n\n## 1.1.0\n\nPrevious release.");
        let notes = parse::document(&text,&source).unwrap();
        assert_eq!(notes.len(),2);
        assert!(notes[0].body.contains(nested));
        assert!(!notes[0].body.contains("Previous release"));
    }
    assert!(parse::document("## 1.2.0\nFixed startup.\n# Unrelated documentation\nOther content.",&source).is_err());
}

#[test]
fn authored_commit_tables_are_content_but_asset_tables_are_not() {
    let body = "### Fixes\n<table><thead><tr><th>Commit</th><th>Description</th></tr></thead><tbody><tr><td>abc123</td><td>Fix startup on Windows</td></tr></tbody></table>";
    let normalized = parse::release_markdown(body, "https://example.com/notes").unwrap();
    assert!(normalized.contains("Fix startup on Windows"));
    assert!(parse::authored(&normalized));
    assert!(!parse::authored(
        "| Download | Checksum |\n|---|---|\n| Windows installer archive | abc123 |"
    ));
    assert_eq!(
        parse::release_markdown("- Fixed a real bug", "https://example.com").unwrap(),
        "- Fixed a real bug"
    );
}

#[test]
fn json_markdown_selection_keeps_only_the_declared_product_and_rejects_schema_drift() {
    let source = document(
        Format::JsonMarkdown,
        Some("/notes/*/content"),
        r"Postman (?P<version>\d+\.\d+\.\d+)",
    );
    let body = r###"{"notes":[{"content":"## Postman 12.1.0\nFixed startup on Windows"}],"cli":[{"content":"## Postman 99.0.0\nOther product changes"}]}"###;
    let notes = parse::document(body, &source).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].version.as_deref(), Some("12.1.0"));
    assert!(notes[0].authored_notes);
    assert!(parse::document(r#"{"notes":[{"markdown":"Renamed field"}]}"#, &source).is_err());
}

#[test]
fn preformatted_html_preserves_release_boundaries_and_explicit_beta_labels() {
    let source = document(
        Format::Plain,
        Some("pre"),
        r"Version (?P<version>\d+\.\d+)(?: (?P<prerelease>beta \d+))?",
    );
    let notes = parse::document("<nav>Other product</nav><pre>Version 7.30 beta 2\r\nFixed beta startup failures.\r\nVersion 7.23\r\nFixed stable startup failures.</pre>", &source).unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].version.as_deref(), Some("7.30-beta.2"));
    assert!(!notes[0].body.contains("stable"));
    assert!(!notes[1].body.contains("Other product"));
}

#[test]
fn annual_release_sections_stop_before_future_plans_without_creating_a_fake_release() {
    let source = document(
        Format::HtmlEntries,
        Some("main"),
        r"(?:9\.2 Present: (?P<version>\d{4})|9\.3 Future)",
    );
    let notes = parse::document("<main><h4>9.2 Present: 2026</h4><p>Improved font handling this year.</p><h4>9.3 Future</h4><p>Unreleased future plans.</p></main>", &source).unwrap();
    assert_eq!(notes.len(), 1);
    assert!(!notes[0].body.contains("future"));
    assert!(version::Version::parse("2026", Scheme::Numeric).is_ok());
}

#[test]
fn inline_release_prose_is_not_discarded_with_its_version_label() {
    let mut source = document(
        Format::Html,
        Some("main"),
        r"go(?P<version>\d+\.\d+\.\d+)(?: \(released [^)]+\))?\s+(?P<body>.*)",
    );
    if let Source::Document { paragraphs, .. } = &mut source {
        *paragraphs = true;
    }
    let notes=parse::document("<main><p>go1.27.4 (released 2026-11-03) includes compiler and runtime security fixes.</p></main>",&source).unwrap();
    assert!(
        notes[0]
            .body
            .contains("compiler and runtime security fixes")
    );
    assert!(notes[0].authored_notes);
    assert!(
        !notes[0]
            .title
            .contains("compiler and runtime security fixes")
    );
    assert_eq!(
        version::Version::parse("6.1.4+one", Scheme::Numeric).unwrap(),
        version::Version::parse("6.1.4+two", Scheme::Numeric).unwrap()
    );
}

#[test]
fn publication_dates_do_not_use_an_aggregators_first_seen_tooltip() {
    let mut source = document(
        Format::HtmlEntries,
        Some("article"),
        r"(?P<date>\d{4}-\d{2}-\d{2})",
    );
    if let Source::Document {
        exclude,
        paragraphs,
        ..
    } = &mut source
    {
        *exclude = Some(".tooltip".into());
        *paragraphs = true;
    }
    let notes=parse::document("<article><p>2026-06-24</p><div class='tooltip'><p>2026-06-25</p></div><h2>Product update</h2><p>Added a useful product feature.</p></article>",&source).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].date.unwrap().to_string(), "2026-06-24");
    assert!(!notes[0].body.contains("2026-06-25"));
}

#[test]
fn standalone_release_dates_require_explicit_formats_and_do_not_search_arbitrary_body_text() {
    let mut source = document(Format::Markdown, None, r"(?P<version>\d+\.\d+\.\d+)");
    let text = "## 1.2.3\n\nNovember 9, 2026\n\nFixed a startup crash.";
    assert!(parse::document(text, &source).unwrap()[0].date.is_none());
    if let Source::Document { date_formats, .. } = &mut source {
        *date_formats = vec!["%B %d, %Y".into()];
    }
    assert_eq!(
        parse::document(text, &source).unwrap()[0]
            .date
            .unwrap()
            .to_string(),
        "2026-11-09"
    );
    assert!(
        parse::document(
            "## 1.2.3\nFixed a startup crash.\n\nNovember 9, 2026",
            &source
        )
        .unwrap()[0]
            .date
            .is_none()
    );
}

#[test]
#[ignore = "explicit live verification of owner-supplied sources, not an inventory scan"]
fn supplied_sources_return_authored_notes() {
    let cache = std::env::var_os("RELEASESCOUT_CORRECTION_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(".releasescout-cache"));
    let engine = Engine {
        catalog: catalog::bundled().unwrap(),
        fetcher: HttpClient::new(CacheOptions {
            directory: cache,
            public_only: true,
            ..Default::default()
        })
        .unwrap(),
    };
    let requested = std::env::var("RELEASESCOUT_CORRECTION_IDS").unwrap_or_else(|_| "vlc|winrar|wiztree|go|gradle|openssl|surrealdb|postman|angular-cli|dbml-cli|kilocode-cli|cline-cli|firefox|windirstat|texlive|canva".into());
    let mut failed = vec![];
    for id in requested.split('|') {
        let mut package = engine.package(id).unwrap().clone();
        if package.streams.is_empty() {
            let hints = source_discovery::Hints {
                manager: "reviewed".into(),
                source: "reviewed".into(),
                id: id.into(),
                installed: None,
                repository: None,
                directory: None,
                homepage: None,
                release_notes: None,
                downloads: vec![],
                root: None,
                references: package.reference_urls.clone(),
            };
            let result = source_discovery::discover(&engine.fetcher, &hints);
            package.streams = result.streams;
            if package.streams.is_empty() {
                println!("{id}: {:?}", result.attempts);
                failed.push(id.to_string());
                continue;
            }
        }
        for stream in &package.streams {
            let scoped = Engine {
                catalog: Catalog {
                    schema_version: 1,
                    packages: vec![package.clone()],
                },
                fetcher: Borrowed(&engine.fetcher),
            };
            match scoped.history(id, Some(&stream.id), None, false) {
                Ok(page) if page.report.notes.iter().any(|n| n.authored_notes) => println!(
                    "{id}/{}: {} notes; first {:?}; dates {:?}",
                    stream.id,
                    page.report.notes.len(),
                    page.report.notes[0].version,
                    page.report
                        .notes
                        .iter()
                        .take(3)
                        .map(|n| n.date)
                        .collect::<Vec<_>>()
                ),
                result => {
                    println!("{id}/{}: {result:?}", stream.id);
                    failed.push(format!("{id}/{}", stream.id));
                }
            }
        }
    }
    assert!(failed.is_empty(), "Unresolved: {failed:?}");
}
