use releasescout_engine::{
    dependencies,
    http::{Document, Fetcher},
    *,
};
use serde_json::json;
use std::{cell::RefCell, collections::HashMap, path::Path, rc::Rc};

fn manifest(path: &Path, value: serde_json::Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, value.to_string()).unwrap();
}

#[test]
fn monorepo_declarations_remain_distinct_from_installed_versions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    manifest(
        &root.join("package.json"),
        json!({"name":"workspace","private":true,"dependencies":{"next":"^16.0.0","alias":"npm:@scope/actual@~2.0.0","local":"workspace:*"},"devDependencies":{"next":"latest"},"peerDependencies":{"react":"^19"}}),
    );
    manifest(
        &root.join("apps/site/package.json"),
        json!({"name":"site","dependencies":{"next":"^15.0.0","local":"*"}}),
    );
    manifest(
        &root.join("packages/local/package.json"),
        json!({"name":"local","private":true}),
    );
    manifest(
        &root.join("node_modules/next/package.json"),
        json!({"name":"next","version":"16.0.1","repository":{"url":"https://github.com/vercel/next.js","directory":"packages/next"}}),
    );
    manifest(
        &root.join("node_modules/alias/package.json"),
        json!({"name":"@scope/actual","version":"2.0.2"}),
    );
    manifest(
        &root.join("dist/package.json"),
        json!({"name":"generated","dependencies":{"should-not-appear":"1.0.0"}}),
    );
    let scan = dependencies::scan(root).unwrap();
    assert_eq!(scan.projects.len(), 3);
    assert_eq!(scan.dependencies.len(), 7);
    assert!(
        scan.dependencies
            .iter()
            .filter(|d| d.package == "next")
            .all(|d| d.installed.as_deref() == Some("16.0.1"))
    );
    let alias = scan
        .dependencies
        .iter()
        .find(|d| d.alias == "alias")
        .unwrap();
    assert_eq!(alias.package, "@scope/actual");
    assert_eq!(alias.installed.as_deref(), Some("2.0.2"));
    assert!(
        scan.dependencies
            .iter()
            .filter(|d| d.package == "local")
            .all(|d| !d.public_registry)
    );
    let ids: std::collections::HashSet<_> = scan.dependencies.iter().map(|d| &d.id).collect();
    assert_eq!(ids.len(), scan.dependencies.len());
    let second = dependencies::scan(root).unwrap();
    assert_eq!(
        scan.dependencies.iter().map(|d| &d.id).collect::<Vec<_>>(),
        second
            .dependencies
            .iter()
            .map(|d| &d.id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn malformed_and_missing_manifests_report_warnings_and_never_become_versions() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("package.json"), "{").unwrap();
    let scan = dependencies::scan(temp.path()).unwrap();
    assert!(scan.dependencies.is_empty());
    assert!(scan.warnings.len() >= 2);
    manifest(
        &temp.path().join("package.json"),
        json!({"dependencies":{"missing":"^3.0.0","../escape":"1.0.0","git":"github:vendor/repo","file":"file:../private"},"devDependencies":7}),
    );
    let scan = dependencies::scan(temp.path()).unwrap();
    assert_eq!(scan.dependencies.len(), 3);
    assert!(scan.dependencies.iter().all(|d| d.installed.is_none()));
    assert!(
        !scan
            .dependencies
            .iter()
            .find(|d| d.alias == "git")
            .unwrap()
            .public_registry
    );
    assert_eq!(scan.warnings.len(), 2);
    assert!(dependencies::scan(&temp.path().join("missing")).is_err());
    assert!(dependencies::scan(&temp.path().join("package.json")).is_err());
}

#[test]
fn unknown_nearer_installation_does_not_inherit_an_ancestors_version() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    manifest(
        &root.join("package.json"),
        json!({"dependencies":{"next":"^16.0.0"}}),
    );
    manifest(
        &root.join("apps/web/package.json"),
        json!({"dependencies":{"next":"^15.0.0"}}),
    );
    manifest(
        &root.join("node_modules/next/package.json"),
        json!({"name":"next","version":"16.0.1"}),
    );
    std::fs::create_dir_all(root.join("apps/web/node_modules/next")).unwrap();
    let scan = dependencies::scan(root).unwrap();
    assert_eq!(
        scan.dependencies
            .iter()
            .filter(|d| d.installed.as_deref() == Some("16.0.1"))
            .count(),
        1
    );
    assert!(
        scan.dependencies
            .iter()
            .find(|d| d.requirement == "^15.0.0")
            .unwrap()
            .installed
            .is_none()
    );
}

#[derive(Default)]
struct Fake {
    pages: HashMap<String, String>,
    links: HashMap<String, String>,
    calls: Rc<RefCell<Vec<String>>>,
}
impl Fetcher for Fake {
    fn get(&self, url: &str) -> Result<Document> {
        self.calls.borrow_mut().push(url.into());
        self.pages
            .get(url)
            .map(|body| Document {
                body: body.clone(),
                headers: self
                    .links
                    .get(url)
                    .map(|link| [("link".into(), link.clone())].into_iter().collect())
                    .unwrap_or_default(),
                evidence: Evidence {
                    url: url.into(),
                    fetched_at: "2026-11-11T00:00:00Z".into(),
                    cache: "fixture".into(),
                },
            })
            .ok_or_else(|| Error::Source("fixture missing".into()))
    }
}

#[test]
fn private_and_local_dependencies_do_not_probe_public_registries_or_names() {
    let temp = tempfile::tempdir().unwrap();
    manifest(
        &temp.path().join("package.json"),
        json!({"dependencies":{"next":"file:./private"}}),
    );
    let dep = dependencies::scan(temp.path())
        .unwrap()
        .dependencies
        .remove(0);
    let fetch = Fake::default();
    let calls = fetch.calls.clone();
    // A local alias named next must not acquire the public Next.js source profile.
    assert!(dependencies::notes(fetch, &dep, Channel::Stable).is_err());
    assert!(calls.borrow().is_empty());
}

#[test]
fn public_registry_requires_exact_package_identity() {
    let temp = tempfile::tempdir().unwrap();
    manifest(
        &temp.path().join("package.json"),
        json!({"dependencies":{"@vendor/tool":"^2.0.0"}}),
    );
    let dep = dependencies::scan(temp.path())
        .unwrap()
        .dependencies
        .remove(0);
    let url = "https://registry.npmjs.org/@vendor%2Ftool/latest";
    let fetch=Fake { pages:HashMap::from([(url.into(),json!({"name":"wrong-package","version":"2.0.0","repository":"https://github.com/wrong/repo"}).to_string())]),..Default::default() };
    let calls = fetch.calls.clone();
    let error = dependencies::notes(fetch, &dep, Channel::Stable)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("identity/version mismatch"),
        "{error}; calls {:?}",
        calls.borrow()
    );
    assert_eq!(calls.borrow().as_slice(), [url]);
}

#[test]
fn uncurated_package_uses_shared_scoped_changelog_discovery_not_repo_wide_notes() {
    let temp = tempfile::tempdir().unwrap();
    manifest(
        &temp.path().join("package.json"),
        json!({"dependencies":{"@vendor/tool":"^2.0.0"}}),
    );
    let dep = dependencies::scan(temp.path())
        .unwrap()
        .dependencies
        .remove(0);
    let pages=HashMap::from([
        ("https://registry.npmjs.org/@vendor%2Ftool/latest".into(),json!({"name":"@vendor/tool","version":"2.0.0","repository":{"url":"https://github.com/vendor/monorepo","directory":"packages/tool"}}).to_string()),
        ("https://raw.githubusercontent.com/vendor/monorepo/HEAD/packages/tool/package.json".into(),json!({"name":"@vendor/tool","version":"2.0.0"}).to_string()),
        ("https://raw.githubusercontent.com/vendor/monorepo/HEAD/packages/tool/CHANGELOG.md".into(),"# Changes\n## 2.0.0\nFixed the project package's parser.\n## 1.0.0\nInitial release.".into()),
    ]);
    let fetch = Fake {
        pages: pages.clone(),
        ..Default::default()
    };
    let calls = fetch.calls.clone();
    let r = dependencies::notes(fetch, &dep, Channel::Stable).unwrap();
    assert!(
        r.notes
            .iter()
            .any(|n| n.version.as_deref() == Some("2.0.0") && n.authored_notes)
    );
    assert!(
        !calls
            .borrow()
            .iter()
            .any(|u| u.starts_with("https://api.github.com/repos/vendor/monorepo/releases"))
    );
    let mut fetch = Fake {
        pages,
        ..Default::default()
    };
    fetch.pages.insert(
        "https://raw.githubusercontent.com/vendor/monorepo/HEAD/packages/tool/package.json".into(),
        json!({"name":"wrong-package","version":"2.0.0"}).to_string(),
    );
    assert!(dependencies::notes(fetch, &dep, Channel::Stable).is_err());
}

#[test]
fn depth_and_manifest_size_are_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let deep = root.join((0..14).map(|_| "folder").collect::<Vec<_>>().join("/"));
    manifest(&deep.join("package.json"), json!({"name":"too-deep"}));
    let body = " ".repeat(1024 * 1024 + 1);
    std::fs::write(root.join("package.json"), body).unwrap();
    let scan = dependencies::scan(root).unwrap();
    assert!(scan.projects.is_empty());
    assert!(scan.warnings.iter().any(|w| w.contains("1 MiB")));
    assert!(scan.warnings.iter().any(|w| w.contains("Depth limit")));
}

#[test]
fn package_channels_filter_history_and_skip_only_bounded_preview_pages() {
    let temp = tempfile::tempdir().unwrap();
    manifest(
        &temp.path().join("package.json"),
        json!({"dependencies":{"next":"latest"}}),
    );
    let dep = dependencies::scan(temp.path())
        .unwrap()
        .dependencies
        .remove(0);
    let release = |version: &str, preview: bool| json!({"tag_name":format!("v{version}"),"prerelease":preview,"draft":false,"name":version,"html_url":format!("https://github.com/vercel/next.js/releases/tag/v{version}"),"body":"Fixed the framework renderer.","published_at":"2026-11-11T00:00:00Z"});
    let first = "https://api.github.com/repos/vercel/next.js/releases?per_page=20&page=1";
    let second = "https://api.github.com/repos/vercel/next.js/releases?per_page=20&page=2";
    for (channel, version, count) in [
        (Channel::Stable, "16.0.1", 2),
        (Channel::Preview, "17.0.0-canary.1", 1),
        (Channel::All, "17.0.0-canary.1", 1),
    ] {
        let fetch = Fake {
            pages: HashMap::from([
                (
                    first.into(),
                    json!([release("17.0.0-canary.1", true)]).to_string(),
                ),
                (second.into(), json!([release("16.0.1", false)]).to_string()),
            ]),
            links: HashMap::from([(first.into(), format!("<{second}>; rel=\"next\""))]),
            ..Default::default()
        };
        let calls = fetch.calls.clone();
        let r = dependencies::notes(fetch, &dep, channel).unwrap();
        assert_eq!(r.notes.len(), 1);
        assert_eq!(r.notes[0].version.as_deref(), Some(version));
        assert_eq!(calls.borrow().len(), count);
    }
}

#[test]
#[ignore = "explicit live package-source check"]
fn live_public_package_history() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["next", "react", "react-dom", "vue", "nuxt", "clsx"] {
        manifest(
            &temp.path().join("package.json"),
            json!({"dependencies":{name:"latest"}}),
        );
        let dep = dependencies::scan(temp.path())
            .unwrap()
            .dependencies
            .remove(0);
        let fetch = http::HttpClient::new(http::CacheOptions {
            directory: temp.path().join("cache"),
            public_only: true,
            ..Default::default()
        })
        .unwrap();
        let r = dependencies::notes(fetch, &dep, Channel::Stable).unwrap();
        assert!(r.notes.iter().any(|n| n.authored_notes));
        assert!(r.notes.iter().all(|n| !n.preview));
        assert!(!r.complete);
        assert!(r.scope.contains("declared latest / installed unresolved"));
        println!("{name}: {} notes / {}", r.notes.len(), r.scope);
    }
}
