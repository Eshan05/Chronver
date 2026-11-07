use releasescout_engine::{Error, Result, icons::*};
use std::{cell::RefCell, collections::HashMap, io::Cursor};
use url::Url;
#[derive(Default)]
struct Fake {
    pages: HashMap<String, Vec<u8>>,
    calls: RefCell<Vec<String>>,
}
impl Fetch for Fake {
    fn get(&self, url: &Url) -> Result<Reply> {
        self.calls.borrow_mut().push(url.to_string());
        Ok(Reply {
            url: url.clone(),
            body: self
                .pages
                .get(url.as_str())
                .ok_or_else(|| Error::Source("missing fixture".into()))?
                .clone(),
        })
    }
}
fn png() -> Vec<u8> {
    let mut out = Cursor::new(vec![]);
    image::DynamicImage::new_rgba8(8, 8)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}
#[test]
fn explicit_product_icon_wins_and_is_cached_with_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let fetch = Fake {
        pages: HashMap::from([("https://assets.example.com/logo.png".into(), png())]),
        ..Default::default()
    };
    let r = Resolver {
        directory: dir.path().into(),
        fetch,
    };
    let plan = Plan {
        icon_url: Some("https://assets.example.com/logo.png".into()),
        homepage: Some("https://example.com/".into()),
    };
    for _ in 0..2 {
        assert_eq!(
            r.resolve(&plan).unwrap().source,
            "https://assets.example.com/logo.png"
        );
    }
    assert_eq!(r.fetch.calls.borrow().len(), 1);
}
#[test]
fn website_icons_use_link_metadata_then_root_favicon_not_owner_avatars() {
    let dir = tempfile::tempdir().unwrap();
    let fetch = Fake {
        pages: HashMap::from([
            (
                "https://example.com/docs/".into(),
                br#"<link rel="shortcut icon" href="../logo.png">"#.to_vec(),
            ),
            ("https://example.com/logo.png".into(), png()),
        ]),
        ..Default::default()
    };
    let r = Resolver {
        directory: dir.path().into(),
        fetch,
    };
    let plan = Plan {
        homepage: Some("https://example.com/docs/".into()),
        ..Default::default()
    };
    assert_eq!(
        r.resolve(&plan).unwrap().source,
        "https://example.com/logo.png"
    );
    assert_eq!(r.fetch.calls.borrow().len(), 2);
    assert!(product_homepage("https://github.com/owner/project").is_none());
    assert!(product_homepage("https://www.npmjs.com/package/tool").is_none());
}
#[test]
fn failures_are_negatively_cached_and_invalid_images_do_not_become_icons() {
    let dir = tempfile::tempdir().unwrap();
    let r = Resolver {
        directory: dir.path().into(),
        fetch: Fake::default(),
    };
    let plan = Plan {
        homepage: Some("https://example.com/".into()),
        ..Default::default()
    };
    assert!(r.resolve(&plan).is_none());
    let count = r.fetch.calls.borrow().len();
    assert!(r.resolve(&plan).is_none());
    assert_eq!(r.fetch.calls.borrow().len(), count);
    let bad = Plan {
        icon_url: Some("https://example.com/not-image".into()),
        ..Default::default()
    };
    r.fetch.calls.borrow_mut().clear();
    assert!(r.resolve(&bad).is_none());
}

#[test]
fn disk_only_reads_preserve_expired_icons_and_failed_refreshes() {
    let dir = tempfile::tempdir().unwrap();
    let plan = Plan {
        icon_url: Some("https://example.com/icon.png".into()),
        homepage: None,
    };
    let mut resolver = Resolver {
        directory: dir.path().into(),
        fetch: Fake {
            pages: HashMap::from([("https://example.com/icon.png".into(), png())]),
            ..Default::default()
        },
    };
    resolver.resolve(&plan).unwrap();
    // This resolver cannot make requests: () doesn't implement Fetch.
    let disk = Resolver {
        directory: dir.path().into(),
        fetch: (),
    };
    assert!(matches!(
        disk.cached(&plan),
        Cached::Hit { fresh: true, .. }
    ));
    let meta = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
    let mut entry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&meta).unwrap()).unwrap();
    entry["checked"] = (chrono::Utc::now().timestamp() - 31 * 86400).into();
    std::fs::write(&meta, serde_json::to_vec(&entry).unwrap()).unwrap();
    assert!(matches!(
        disk.cached(&plan),
        Cached::Hit { fresh: false, .. }
    ));
    resolver.fetch.pages.clear();
    assert_eq!(
        resolver.resolve(&plan).unwrap().source,
        "https://example.com/icon.png"
    );
    assert!(matches!(
        disk.cached(&plan),
        Cached::Hit { fresh: false, .. }
    ));
    std::fs::write(meta.with_extension("png"), b"broken").unwrap();
    assert!(matches!(disk.cached(&plan), Cached::Miss));
    assert!(matches!(disk.cached(&Plan::default()), Cached::Miss));
}

#[test]
fn disk_only_negative_entries_do_not_hide_other_plans() {
    let dir = tempfile::tempdir().unwrap();
    let resolver = Resolver {
        directory: dir.path().into(),
        fetch: Fake::default(),
    };
    let plan = Plan {
        homepage: Some("https://example.com/".into()),
        ..Default::default()
    };
    assert!(resolver.resolve(&plan).is_none());
    let disk = Resolver {
        directory: dir.path().into(),
        fetch: (),
    };
    assert!(matches!(disk.cached(&plan), Cached::Negative));
    assert!(matches!(
        disk.cached(&Plan {
            homepage: Some("https://other.example.com/".into()),
            ..Default::default()
        }),
        Cached::Miss
    ));
}
#[test]
fn network_url_policy_rejects_local_schemes_credentials_and_literal_addresses() {
    for url in [
        "http://example.com/icon",
        "file:///C:/icon.png",
        "https://127.0.0.1/a",
        "https://[::1]/a",
        "https://app.local/a",
        "https://user:pass@example.com/a",
        "https://example.com:8443/a",
    ] {
        assert!(safe_url(url).is_none(), "{url}");
    }
}

#[test]
#[ignore = "explicit public-network icon/cache check"]
fn live_official_favicon_is_decoded_and_cached() {
    let directory = tempfile::tempdir().unwrap();
    let r = Resolver {
        directory: directory.path().into(),
        fetch: Network,
    };
    for site in ["https://www.python.org/", "https://supabase.com/"] {
        let plan = Plan {
            homepage: Some(site.into()),
            ..Default::default()
        };
        let result = r.resolve(&plan).expect("official icon unavailable");
        assert!(image::load_from_memory(&result.png).is_ok());
        assert!(result.source.starts_with("https://"));
        println!("Resolved {}", result.source);
        assert!(r.resolve(&plan).is_some());
    }
}
