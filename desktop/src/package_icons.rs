use crate::model::{Package, State};
use releasescout_engine::{
    http::{CacheOptions, Fetcher, HttpClient},
    icons::{Plan, product_homepage},
    watchlist::Ecosystem,
};
use rune::Latest;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

#[derive(Default)]
pub struct Loader {
    pub work: Latest<(String, Option<Plan>)>,
    attempted: HashSet<String>,
    resolved: HashMap<String, Plan>,
}

fn name(p: &Package) -> Option<&str> {
    if let Some(entry) = &p.watchlist {
        return (entry.ecosystem == Ecosystem::Npm).then_some(entry.name.as_str());
    }
    p.dependency
        .as_ref()
        .filter(|d| d.public_registry)
        .map(|d| d.package.as_str())
}

// Exact package identities only; never infer a brand from an npm scope or GitHub owner.
fn brand(name: &str) -> Option<&'static str> {
    Some(match name {
        "next" => "https://nextjs.org/",
        "react" | "react-dom" => "https://react.dev/",
        "nuxt" => "https://nuxt.com/",
        "vue" => "https://vuejs.org/",
        "typescript" => "https://www.typescriptlang.org/",
        "vite" => "https://vite.dev/",
        "tailwindcss" => "https://tailwindcss.com/",
        "zod" => "https://zod.dev/",
        "prisma" | "@prisma/client" => "https://www.prisma.io/",
        _ => return None,
    })
}

pub fn initial_plan(p: &Package) -> Option<Plan> {
    let name = name(p)?;
    let homepage = p
        .dependency
        .as_ref()
        .and_then(|d| d.homepage.as_deref())
        .and_then(product_homepage)
        .or_else(|| brand(name).map(String::from))?;
    Some(Plan {
        homepage: Some(homepage),
        icon_url: None,
    })
}

fn metadata(fetch: &impl Fetcher, name: &str) -> Option<Plan> {
    if !releasescout_engine::dependencies::package_name(name) {
        return None;
    }
    let mut url = url::Url::parse("https://registry.npmjs.org/").ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .push(name)
        .push("latest");
    let document = fetch.get(url.as_str()).ok()?;
    let value: serde_json::Value = serde_json::from_str(&document.body).ok()?;
    if value["name"].as_str() != Some(name) {
        return None;
    }
    let homepage = value["homepage"]
        .as_str()
        .and_then(product_homepage)
        .or_else(|| brand(name).map(String::from))?;
    Some(Plan {
        homepage: Some(homepage),
        icon_url: None,
    })
}

impl State {
    pub fn poll_package_icons(&mut self) -> bool {
        if !self.icon_loader.enabled {
            return false;
        }
        let mut changed = false;
        if let Some((name, Some(plan))) = self.package_icons.work.take() {
            self.package_icons.resolved.insert(name, plan);
        }
        for p in &mut self.catalog.packages {
            if p.icon_plan.is_none() {
                let plan = name(p)
                    .and_then(|n| self.package_icons.resolved.get(n))
                    .cloned()
                    .or_else(|| initial_plan(p));
                if let Some(plan) = plan {
                    p.icon_plan = Some(plan);
                    changed = true;
                }
            }
        }
        if self.icon_loader.network_enabled && !self.package_icons.work.is_pending() {
            let name = crate::icon_loader::priority_packages(self)
                .into_iter()
                .filter_map(|i| self.catalog.packages.get(i))
                .filter(|p| {
                    p.icon_plan.is_none() && p.custom_icon.is_none() && p.custom_symbol.is_none()
                })
                .filter_map(name)
                .find(|name| !self.package_icons.attempted.contains(*name))
                .map(String::from);
            if let Some(name) = name {
                self.package_icons.attempted.insert(name.clone());
                let options = CacheOptions {
                    directory: crate::live::cache_directory(),
                    public_only: true,
                    ttl: Duration::from_secs(7 * 24 * 3600),
                    ..Default::default()
                };
                self.package_icons.work.request(name, move |name| {
                    let plan = HttpClient::new(options)
                        .ok()
                        .and_then(|fetch| metadata(&fetch, &name));
                    (name, plan)
                });
            }
        }
        changed
    }
}

pub fn cached_scene(s: &mut State) {
    use releasescout_engine::watchlist::{Entry, Group};
    s.packages.watch.entries = ["next", "react", "vue", "icon-fallback-fixture"]
        .into_iter()
        .map(|name| Entry {
            name: name.into(),
            ecosystem: Ecosystem::Npm,
            group: Group::Core,
        })
        .collect();
    s.sync_watchlist();
    s.set_section(crate::model::LibrarySection::Packages);
    s.icon_loader.enabled = true;
    s.icon_loader.network_enabled = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use releasescout_engine::{Result, http::Document};
    struct Fixture(serde_json::Value);
    impl Fetcher for Fixture {
        fn get(&self, url: &str) -> Result<Document> {
            assert_eq!(url, "https://registry.npmjs.org/@fixture%2Fwidget/latest");
            Ok(Document {
                body: self.0.to_string(),
                headers: Default::default(),
                evidence: releasescout_engine::Evidence {
                    url: url.into(),
                    fetched_at: "2026-11-14T00:00:00Z".into(),
                    cache: "fixture".into(),
                },
            })
        }
    }

    #[test]
    fn metadata_is_exact_scoped_and_never_uses_registry_or_owner_icons() {
        let fetch = Fixture(
            serde_json::json!({"name":"@fixture/widget","homepage":"https://widgets.example/docs"}),
        );
        assert_eq!(
            metadata(&fetch, "@fixture/widget")
                .unwrap()
                .homepage
                .as_deref(),
            Some("https://widgets.example/docs")
        );
        assert!(
            metadata(
                &Fixture(serde_json::json!({"name":"other","homepage":"https://wrong.example"})),
                "@fixture/widget"
            )
            .is_none()
        );
        for homepage in [
            "https://github.com/acme/widget",
            "https://www.npmjs.com/package/widget",
            "http://127.0.0.1/",
            "https://localhost/",
            "https://user:secret@widgets.example/",
        ] {
            assert!(
                metadata(
                    &Fixture(serde_json::json!({"name":"@fixture/widget","homepage":homepage})),
                    "@fixture/widget"
                )
                .is_none()
            );
        }
        assert!(brand("@next/plugin").is_none());
        assert!(brand("next-extra").is_none());
    }

    #[test]
    fn watchlist_icons_share_plans_and_private_declarations_do_not_get_public_lookups() {
        use releasescout_engine::watchlist::{Entry, Group};
        let mut s = State::new();
        s.packages.watch.entries = ["react", "react-dom", "@fixture/widget"]
            .into_iter()
            .map(|name| Entry {
                name: name.into(),
                ecosystem: Ecosystem::Npm,
                group: Group::Core,
            })
            .collect();
        s.sync_watchlist();
        let watch: Vec<_> = s
            .catalog
            .packages
            .iter()
            .filter(|p| p.watchlist.is_some())
            .collect();
        assert_eq!(watch[0].icon_plan, watch[1].icon_plan);
        assert!(
            crate::sidebar_filters::source(&s, watch[0]),
            "Reviewed package sources are found before notes are fetched"
        );
        assert!(!crate::sidebar_filters::notes(&s, watch[0]));
        assert_eq!(
            watch[0].icon_plan.as_ref().unwrap().homepage.as_deref(),
            Some("https://react.dev/")
        );
        assert!(watch[2].icon_plan.is_none());
        assert!(
            !s.poll_package_icons(),
            "Test/headless runs never fetch metadata"
        );
        assert!(!s.package_icons.work.is_pending());
        let mut private = watch_package();
        private.dependency = Some(releasescout_engine::dependencies::Dependency {
            id: "private".into(),
            ecosystem: releasescout_engine::dependencies::Ecosystem::Npm,
            project: "project".into(),
            manifest: "package.json".into(),
            package: "next".into(),
            alias: "next".into(),
            requirement: "workspace:*".into(),
            kind: "dependency".into(),
            installed: None,
            root: None,
            repository: None,
            directory: None,
            homepage: Some("https://private.example".into()),
            public_registry: false,
        });
        assert!(name(&private).is_none());
        assert!(initial_plan(&private).is_none());
        private.dependency.as_mut().unwrap().public_registry = true;
        assert_eq!(
            initial_plan(&private).unwrap().homepage.as_deref(),
            Some("https://private.example/")
        );
    }

    fn watch_package() -> Package {
        serde_json::from_value(serde_json::json!({"id":"test","name":"next"})).unwrap()
    }

    #[test]
    #[ignore = "explicit public metadata/icon check, warms the shared icon cache"]
    fn live_package_icons_resolve_and_are_cached() {
        use releasescout_engine::icons::{Cached, Network, Resolver};
        let fetch = HttpClient::new(CacheOptions {
            directory: crate::live::cache_directory(),
            public_only: true,
            ..Default::default()
        })
        .unwrap();
        let resolver = Resolver {
            directory: crate::live::cache_directory().with_file_name("icons"),
            fetch: Network,
        };
        for name in ["next", "react", "vue", "@tanstack/react-query"] {
            let plan =
                metadata(&fetch, name).unwrap_or_else(|| panic!("No product homepage for {name}"));
            let icon = resolver
                .resolve(&plan)
                .unwrap_or_else(|| panic!("No icon for {name}: {plan:?}"));
            let image = rune::ImageCache::new(std::sync::Arc::new(|| {})).load(icon.png);
            image.wait().unwrap();
            assert!(image.size().is_some());
            assert!(matches!(
                resolver.cached(&plan),
                Cached::Hit { fresh: true, .. }
            ));
            eprintln!("{name}: {} (decoded and cached)", icon.source);
        }
    }
}
