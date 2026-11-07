use releasescout_engine::{
    catalog,
    http::{Document, Fetcher},
    *,
};
use std::{cell::RefCell, collections::HashMap};
struct Fake {
    pages: HashMap<String, (String, Option<String>)>,
    calls: RefCell<Vec<String>>,
}
impl Fetcher for Fake {
    fn get(&self, url: &str) -> Result<Document> {
        self.calls.borrow_mut().push(url.into());
        let (body, link) = self
            .pages
            .get(url)
            .ok_or_else(|| Error::Source("fixture miss".into()))?;
        Ok(Document {
            body: body.clone(),
            headers: link.iter().map(|l| ("link".into(), l.clone())).collect(),
            evidence: Evidence {
                url: url.into(),
                fetched_at: "2026-11-10T00:00:00Z".into(),
                cache: "network".into(),
            },
        })
    }
}
fn engine() -> Engine<Fake> {
    Engine {catalog:catalog::load(&serde_json::json!({"schema_version":1,"packages":[{"id":"test","name":"Test","tracking":"track","identity":"confirmed","streams":[{"id":"main","scope":"test","basis":"version","sources":[{"kind":"github","owner":"a","repo":"b"}]}]}]}).to_string()).unwrap(),fetcher:Fake {pages:HashMap::new(),calls:RefCell::new(vec![])}}
}
fn release(v: &str) -> serde_json::Value {
    serde_json::json!({"tag_name":v,"name":v,"body":"Fixed a real bug","html_url":format!("https://github.com/a/b/releases/tag/{v}"),"published_at":null,"draft":false,"prerelease":false})
}
#[test]
fn github_api_failure_uses_official_atom_without_claiming_complete_channel_coverage() {
    let mut e = engine();
    e.fetcher.pages.insert("https://github.com/a/b/releases.atom".into(),(r#"<feed xmlns="http://www.w3.org/2005/Atom"><id>https://github.com/a/b/releases</id><title>Releases</title><updated>2026-11-10T00:00:00Z</updated><entry><id>tag:one</id><title>1.1.0</title><link href="https://github.com/a/b/releases/tag/v1.1.0"/><updated>2026-11-10T00:00:00Z</updated><content type="html">&lt;p&gt;Fixed a startup crash.&lt;/p&gt;</content></entry></feed>"#.into(),None));
    e.catalog.packages[0].streams[0].sources[0] = Source::Github {
        owner: "a".into(),
        repo: "b".into(),
        tag_prefix: "v".into(),
        tag_pattern: None,
        max_pages: 1,
    };
    let page = e.history("test", None, None, false).unwrap();
    assert_eq!(page.report.notes.len(), 1);
    assert!(page.report.notes[0].authored_notes);
    assert!(!page.report.complete);
    assert_eq!(page.report.notes[0].date, None);
    assert!(page.report.warnings.iter().any(|w| w.contains("Atom")));
    let report = e
        .notes(
            "test",
            None,
            Selection::Versions {
                from: "1.0.0".into(),
                to: "1.1.0".into(),
                channel: Channel::Stable,
            },
            false,
        )
        .unwrap();
    assert!(report.notes.is_empty());
    assert!(!report.complete);
}
#[test]
fn official_atom_redirects_follow_repository_moves_but_not_foreign_feed_hosts() {
    struct Moved {
        foreign: bool,
    }
    impl Fetcher for Moved {
        fn get(&self, url: &str) -> Result<Document> {
            if url.starts_with("https://api.github.com/") {
                return Err(Error::Source("API unavailable".into()));
            }
            Ok(Document{body:r#"<feed xmlns="http://www.w3.org/2005/Atom"><id>releases</id><title>Releases</title><updated>2026-11-10T00:00:00Z</updated><entry><id>tag:one</id><title>1.1.0</title><link href="https://github.com/newowner/b/releases/tag/1.1.0"/><updated>2026-11-10T00:00:00Z</updated><content>Fixed a startup crash.</content></entry></feed>"#.into(),headers:Default::default(),evidence:Evidence{url:if self.foreign{"https://foreign.example/releases.atom"}else{"https://github.com/newowner/b/releases.atom"}.into(),cache:"network".into(),fetched_at:"2026-11-10T00:00:00Z".into()}})
        }
    }
    let catalog = engine().catalog;
    let good = Engine {
        catalog: catalog.clone(),
        fetcher: Moved { foreign: false },
    }
    .history("test", None, None, false)
    .unwrap();
    assert_eq!(good.report.notes.len(), 1);
    assert!(good.report.notes[0].authored_notes);
    assert!(
        Engine {
            catalog,
            fetcher: Moved { foreign: true }
        }
        .history("test", None, None, false)
        .is_err()
    );
}
#[test]
fn sparse_history_tries_declared_fallback_and_keeps_sparse_evidence_if_it_fails() {
    let mut e = engine();
    let first = "https://api.github.com/repos/a/b/releases?per_page=20&page=1";
    let mut sparse = release("1.0.0");
    sparse["body"] = "".into();
    e.fetcher.pages.insert(
        first.into(),
        (serde_json::json!([sparse]).to_string(), None),
    );
    e.catalog.packages[0].streams[0]
        .sources
        .push(releasescout_engine::source_discovery::document(
            "https://example.com/notes.md",
            false,
        ));
    let page = e.history("test", None, None, false).unwrap();
    assert_eq!(page.report.notes.len(), 1);
    assert!(!page.report.notes[0].authored_notes);
    assert_eq!(page.report.attempts.len(), 2);
    e.fetcher.pages.insert(
        "https://example.com/notes.md".into(),
        ("## 1.0.0\n\nFixed a startup crash.".into(), None),
    );
    let page = e.history("test", None, None, false).unwrap();
    assert!(page.report.notes[0].authored_notes);
    assert_eq!(page.report.attempts.len(), 2);
}
#[test]
fn empty_primary_does_not_displace_nonempty_sparse_fallback() {
    let mut e = engine();
    e.fetcher.pages.insert(
        "https://api.github.com/repos/a/b/releases?per_page=20&page=1".into(),
        ("[]".into(), None),
    );
    e.catalog.packages[0].streams[0]
        .sources
        .push(Source::Github {
            owner: "c".into(),
            repo: "d".into(),
            tag_prefix: String::new(),
            tag_pattern: None,
            max_pages: 1,
        });
    let mut sparse = release("1.0.0");
    sparse["body"] = "".into();
    e.fetcher.pages.insert(
        "https://api.github.com/repos/c/d/releases?per_page=20&page=1".into(),
        (serde_json::json!([sparse]).to_string(), None),
    );
    let page = e.history("test", None, None, false).unwrap();
    assert_eq!(page.report.notes.len(), 1);
    assert!(!page.report.complete);
}
#[test]
fn history_fetches_one_twenty_item_page_and_continuation_only_on_request() {
    let mut e = engine();
    let first = "https://api.github.com/repos/a/b/releases?per_page=20&page=1";
    let second = "https://api.github.com/repos/a/b/releases?per_page=20&page=2";
    e.fetcher.pages.insert(first.into(),(serde_json::json!([release("2.0.0")]).to_string(),Some("<https://api.github.com/repositories/12/releases?per_page=20&page=2>; rel=\"next\"".into())));
    e.fetcher.pages.insert(
        second.into(),
        (serde_json::json!([release("1.0.0")]).to_string(), None),
    );
    let page = e.history("test", None, None, false).unwrap();
    assert_eq!(e.fetcher.calls.borrow().len(), 1);
    assert!(page.next.is_some());
    assert!(!page.report.complete);
    let mut invalid = page.next.clone().unwrap();
    invalid.package = "other".into();
    assert!(e.history("test", None, Some(invalid), false).is_err());
    assert_eq!(e.fetcher.calls.borrow().len(), 1);
    let last = e.history("test", None, page.next, false).unwrap();
    assert!(last.next.is_none());
    assert_eq!(last.report.notes[0].version.as_deref(), Some("1.0.0"));
    assert_eq!(e.fetcher.calls.borrow().len(), 2);
}
#[test]
fn empty_filtered_page_still_exposes_older_history() {
    let mut e = engine();
    e.fetcher.pages.insert(
        "https://api.github.com/repos/a/b/releases?per_page=20&page=1".into(),
        (
            "[]".into(),
            Some(
                "<https://api.github.com/repos/a/b/releases?per_page=20&page=2>; rel=\"next\""
                    .into(),
            ),
        ),
    );
    let page = e.history("test", None, None, false).unwrap();
    assert!(page.report.notes.is_empty());
    assert!(page.next.is_some());
}
#[test]
fn unsafe_links_are_never_followed() {
    let mut e = engine();
    e.fetcher.pages.insert(
        "https://api.github.com/repos/a/b/releases?per_page=20&page=1".into(),
        (
            serde_json::json!([release("1.0.0")]).to_string(),
            Some("<https://evil.example/next>; rel=\"next\"".into()),
        ),
    );
    let page = e.history("test", None, None, false).unwrap();
    assert!(page.next.is_none());
    assert!(!page.report.complete);
    assert_eq!(e.fetcher.calls.borrow().len(), 1);
}
#[test]
fn document_pages_reuse_the_same_source_without_returning_all_notes() {
    let mut e = engine();
    e.catalog.packages[0].streams[0].sources = vec![Source::Document {
        url: "https://example.com/notes".into(),
        format: Format::Markdown,
        boundary: r"(?P<version>\d+\.\d+\.\d+)".into(),
        selector: None,
        exclude: None,
        date_formats: vec![],
        paragraphs: false,
    }];
    e.fetcher.pages.insert(
        "https://example.com/notes".into(),
        (
            (0..25)
                .rev()
                .map(|i| format!("## 1.0.{i}\nFixed another real bug\n"))
                .collect(),
            None,
        ),
    );
    let page = e.history("test", None, None, false).unwrap();
    assert_eq!(page.report.notes.len(), 20);
    assert!(page.next.is_some());
    let page = e.history("test", None, page.next, false).unwrap();
    assert_eq!(page.report.notes.len(), 5);
    assert!(page.next.is_none());
}
