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
