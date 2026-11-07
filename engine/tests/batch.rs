use releasescout_engine::{
    batch::{self, Limits},
    catalog,
    http::{Document, Fetcher},
    *,
};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Fake {
    active: AtomicUsize,
    peak: AtomicUsize,
    calls: Mutex<Vec<String>>,
}
impl Fetcher for Fake {
    fn get(&self, url: &str) -> Result<Document> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(if url.ends_with("slow") {
            60
        } else {
            15
        }));
        self.calls.lock().unwrap().push(url.into());
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(Document {
            body: "## 1.1.0\nFixed a regression in the command parser.\n## 1.0.0\nInitial working software release.".into(),
            headers: Default::default(),
            evidence: Evidence { url: url.into(), fetched_at: "2026-11-16T00:00:00Z".into(), cache: "fixture".into() },
        })
    }
}
fn engine() -> Engine<Fake> {
    let packages: Vec<_> = ["slow", "fast"].into_iter().map(|id| serde_json::json!({
        "id":id,"name":id,"tracking":"track","identity":"confirmed","streams":[{
            "id":"main","scope":"Test CLI","basis":"version","sources":[{
                "kind":"document","url":format!("https://example.com/{id}"),"format":"markdown","boundary":r"(?P<version>\d+\.\d+\.\d+)"
            }]
        }]
    })).collect();
    Engine {
        catalog: catalog::load(
            &serde_json::json!({"schema_version":1,"packages":packages}).to_string(),
        )
        .unwrap(),
        fetcher: Fake {
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            calls: Mutex::new(vec![]),
        },
    }
}
fn request() -> batch::Request {
    serde_json::from_value(serde_json::json!({"schema_version":1,"queries": [
        {"key":"first","package":"slow","query":{"kind":"range","from":"1.0.0","to":"1.1.0"}},
        {"key":"bad","package":"missing","query":{"kind":"range","from":"1.0.0","to":"1.1.0"}},
        {"key":"last","package":"fast","query":{"kind":"range","from":"1.0.0","to":"1.1.0"}}
    ]}))
    .unwrap()
}
#[test]
fn concurrency_is_bounded_results_ordered_and_errors_isolated() {
    let e = engine();
    let result = batch::run(&e, request(), 2, Limits::default(), false).unwrap();
    assert_eq!(
        result
            .results
            .iter()
            .map(|r| r.key.as_str())
            .collect::<Vec<_>>(),
        ["first", "bad", "last"]
    );
    assert!(result.failed());
    assert!(!result.complete);
    assert!(
        result.results[0].report.as_ref().unwrap()["complete"]
            .as_bool()
            .unwrap()
    );
    assert!(result.results[1].error.is_some());
    assert!(
        result.results[2].report.as_ref().unwrap()["complete"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(e.fetcher.peak.load(Ordering::SeqCst), 2);
    assert!(result.results[2].elapsed_ms < result.results[0].elapsed_ms);
    let e = engine();
    batch::run(&e, request(), 1, Limits::default(), false).unwrap();
    assert_eq!(e.fetcher.peak.load(Ordering::SeqCst), 1);
}
#[test]
fn malformed_batches_do_no_source_work() {
    let e = engine();
    for mutation in 0..4 {
        let mut q = request();
        match mutation {
            0 => q.queries[1].key = q.queries[0].key.clone(),
            1 => q.schema_version = 2,
            2 => q.queries.clear(),
            _ => q.queries[0].key.clear(),
        }
        assert!(batch::run(&e, q, 2, Limits::default(), false).is_err());
    }
    assert!(batch::run(&e, request(), 9, Limits::default(), false).is_err());
    assert!(
        batch::run(
            &e,
            request(),
            2,
            Limits {
                body_bytes: 0,
                ..Limits::default()
            },
            false
        )
        .is_err()
    );
    assert!(e.fetcher.calls.lock().unwrap().is_empty());
}
