use crate::model::State;
use releasescout_engine::icons::{Cached, Network, Plan, Resolved, Resolver};
use rune::Latest;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

pub struct Loader {
    pub enabled: bool,
    pub network_enabled: bool,
    pub directory: PathBuf,
    pub cache: Latest<Vec<(Plan, Cached)>>,
    pub workers: [Latest<(Plan, Option<Resolved>)>; 2],
    cache_checked: HashSet<Plan>,
    refresh: HashSet<Plan>,
    attempted: HashSet<Plan>,
    loaded: HashMap<Plan, (rune::Image, String)>,
}
impl Default for Loader {
    fn default() -> Self {
        Self {
            enabled: false,
            network_enabled: false,
            directory: crate::live::cache_directory().with_file_name("icons"),
            cache: Latest::default(),
            workers: Default::default(),
            cache_checked: HashSet::new(),
            refresh: HashSet::new(),
            attempted: HashSet::new(),
            loaded: HashMap::new(),
        }
    }
}

pub(crate) fn priority_packages(s: &State) -> Vec<usize> {
    let mut seen = HashSet::new();
    s.visible
        .iter()
        .filter_map(|&i| s.rows.get(i))
        .find(|r| Some(r.id.as_str()) == s.table.cursor())
        .map(|r| r.package)
        .into_iter()
        .chain(
            s.sidebar_scroll
                .visible_rows
                .clone()
                .filter_map(|i| crate::sidebar_groups::row_index(s, i))
                .filter_map(|i| s.visible.get(i))
                .filter_map(|&i| s.rows.get(i))
                .map(|r| r.package),
        )
        .chain(
            crate::sidebar_groups::shown(s)
                .iter()
                .map(|&i| s.rows[i].package),
        )
        .filter(|&i| seen.insert(i))
        .collect()
}

fn needs_remote(p: &crate::model::Package) -> bool {
    p.custom_symbol.is_none()
        && p.custom_icon.as_ref().is_none_or(|i| i.error().is_some())
        && (p.icon.as_ref().is_none_or(|i| i.error().is_some())
            || p.icon_source
                .as_ref()
                .is_some_and(|s| s.starts_with("https://")))
}

impl State {
    pub fn poll_remote_icons(&mut self) -> bool {
        if !self.icon_loader.enabled {
            return false;
        }
        let mut changed = false;
        if let Some(entries) = self.icon_loader.cache.take() {
            changed = true;
            for (plan, cached) in entries {
                self.icon_loader.cache_checked.insert(plan.clone());
                match cached {
                    Cached::Hit { resolved, fresh } => {
                        if !fresh {
                            self.icon_loader.refresh.insert(plan.clone());
                        }
                        self.icon_loader
                            .loaded
                            .insert(plan, (self.icons.load(resolved.png), resolved.source));
                    }
                    Cached::Miss => {
                        self.icon_loader.refresh.insert(plan);
                    }
                    Cached::Negative => {}
                }
            }
        }
        for worker in &self.icon_loader.workers {
            if let Some((plan, Some(resolved))) = worker.take() {
                let image = self.icons.load(resolved.png);
                self.icon_loader
                    .loaded
                    .insert(plan, (image, resolved.source));
                changed = true;
            }
        }
        let native: HashMap<_, _> = self
            .catalog
            .packages
            .iter()
            .filter_map(|p| {
                let icon = p.icon.as_ref()?;
                let source = p.icon_source.as_ref()?;
                if icon.error().is_some() || source.starts_with("https://") {
                    return None;
                }
                Some((p.group_key.clone()?, (icon.clone(), source.clone())))
            })
            .collect();
        for p in &mut self.catalog.packages {
            if let Some((image, source)) = p.group_key.as_ref().and_then(|g| native.get(g))
                && p.icon.as_ref().is_none_or(|i| i.id() != image.id())
            {
                p.icon = Some(image.clone());
                p.icon_source = Some(source.clone());
                changed = true;
            }
            if (p.icon.is_none()
                || p.icon.as_ref().is_some_and(|i| i.error().is_some())
                || p.icon_source
                    .as_ref()
                    .is_some_and(|s| s.starts_with("https://")))
                && let Some(plan) = &p.icon_plan
                && let Some((icon, source)) = self.icon_loader.loaded.get(plan)
                && (icon.size().is_some() || p.icon.as_ref().is_none_or(|i| i.size().is_none()))
                && (p
                    .icon
                    .as_ref()
                    .is_none_or(|current| current.id() != icon.id())
                    || p.icon_source.as_deref() != Some(source.as_str()))
            {
                p.icon = Some(icon.clone());
                p.icon_source = Some(source.clone());
                changed = true;
            }
        }
        let packages = priority_packages(self);
        if !self.icon_loader.cache.is_pending() {
            let mut unique = HashSet::new();
            let plans: Vec<_> = packages
                .iter()
                .filter_map(|&i| {
                    let p = &self.catalog.packages[i];
                    if !needs_remote(p) {
                        return None;
                    }
                    p.icon_plan
                        .as_ref()
                        .filter(|plan| {
                            !plan.is_empty()
                                && !self.icon_loader.cache_checked.contains(*plan)
                                && !self.icon_loader.loaded.contains_key(*plan)
                                && unique.insert((*plan).clone())
                        })
                        .cloned()
                })
                .take(16)
                .collect();
            if !plans.is_empty() {
                let directory = self.icon_loader.directory.clone();
                self.icon_loader.cache.request(plans, move |plans| {
                    let resolver = Resolver {
                        directory,
                        fetch: (),
                    };
                    plans
                        .into_iter()
                        .map(|plan| {
                            let cached = resolver.cached(&plan);
                            (plan, cached)
                        })
                        .collect()
                });
            }
        }
        if !self.icon_loader.network_enabled {
            return changed;
        }
        for worker in &self.icon_loader.workers {
            if worker.is_pending() {
                continue;
            }
            let plan = packages
                .iter()
                .filter_map(|&i| {
                    let p = &self.catalog.packages[i];
                    if !needs_remote(p) {
                        return None;
                    }
                    p.icon_plan
                        .as_ref()
                        .filter(|p| {
                            self.icon_loader.refresh.contains(*p)
                                && !self.icon_loader.attempted.contains(*p)
                        })
                        .cloned()
                })
                .next();
            if let Some(plan) = plan {
                self.icon_loader.attempted.insert(plan.clone());
                let directory = self.icon_loader.directory.clone();
                worker.request(plan, move |plan| {
                    let resolver = Resolver {
                        directory,
                        fetch: Network,
                    };
                    let result = resolver.resolve(&plan);
                    (plan, result)
                });
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_then_onscreen_icons_precede_offscreen_and_stale_ranges_are_safe() {
        let mut s = State::demo();
        let selected = s.rows[s.visible[0]].package;
        let id = s.catalog.packages[selected].id.clone();
        s.select_package(&id);
        s.sidebar_scroll.visible_rows = 8..11;
        let priority = priority_packages(&s);
        assert_eq!(priority[0], selected);
        assert_eq!(
            &priority[1..4],
            &s.visible[8..11]
                .iter()
                .map(|&i| s.rows[i].package)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            priority.iter().collect::<HashSet<_>>().len(),
            priority.len()
        );
        s.sidebar_scroll.visible_rows = 9999..10002;
        assert_eq!(priority_packages(&s)[0], selected);
        crate::sidebar_groups::scene(&mut s, true);
        s.sidebar_scroll.visible_rows = 0..5;
        let shown: HashSet<_> = crate::sidebar_groups::shown(&s)
            .iter()
            .map(|&i| s.rows[i].package)
            .collect();
        assert!(priority_packages(&s).iter().all(|i| shown.contains(i)));
    }

    struct Fixture;
    impl releasescout_engine::icons::Fetch for Fixture {
        fn get(
            &self,
            url: &url::Url,
        ) -> releasescout_engine::Result<releasescout_engine::icons::Reply> {
            Ok(releasescout_engine::icons::Reply {
                url: url.clone(),
                body: include_bytes!("../assets/providers/winget.png").to_vec(),
            })
        }
    }

    #[test]
    fn inventory_refresh_keeps_working_remote_handles_without_another_poll() {
        let mut s = State::new();
        crate::sidebar_info::scene(&mut s);
        let at = s
            .rows
            .iter()
            .find(|r| r.name == "GitHub CLI")
            .unwrap()
            .package;
        let id = s.catalog.packages[at].id.clone();
        let image = s.icons.rgba(1, 1, vec![255, 0, 0, 255]);
        image.wait().unwrap();
        s.catalog.packages[at].icon = Some(image.clone());
        s.catalog.packages[at].icon_source = Some("https://example.com/gh.png".into());
        s.accept_scan(s.inventory.scans[0].clone());
        let p = s.catalog.packages.iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.icon.as_ref().unwrap().id(), image.id());
        assert_eq!(p.icon_source.as_deref(), Some("https://example.com/gh.png"));
    }

    #[test]
    fn disk_hits_load_even_when_both_network_workers_are_blocked() {
        use std::{
            sync::mpsc,
            time::{Duration, Instant},
        };
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan {
            icon_url: Some("https://example.com/cached.png".into()),
            homepage: None,
        };
        Resolver {
            directory: dir.path().into(),
            fetch: Fixture,
        }
        .resolve(&plan)
        .unwrap();
        let mut s = State::demo();
        for p in &mut s.catalog.packages {
            p.icon_plan = None;
        }
        let target = s.rows[s.visible[0]].package;
        s.catalog.packages[target].icon_plan = Some(plan.clone());
        s.icon_loader.directory = dir.path().into();
        s.icon_loader.enabled = true;
        let mut release = vec![];
        for worker in &s.icon_loader.workers {
            let (tx, rx) = mpsc::channel();
            release.push(tx);
            worker.request(Plan::default(), move |plan| {
                let _ = rx.recv_timeout(Duration::from_secs(10));
                (plan, None)
            });
        }
        s.poll_remote_icons();
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.icon_loader.cache.is_pending() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        s.poll_remote_icons();
        let blocked = s.icon_loader.workers.iter().all(|w| w.is_pending());
        for tx in release {
            let _ = tx.send(());
        }
        assert!(
            blocked,
            "a disk hit must not wait for either network worker"
        );
        let icon = s.catalog.packages[target]
            .icon
            .as_ref()
            .expect("disk icon applied");
        icon.wait().unwrap();
        assert!(icon.size().is_some());
        assert_eq!(
            s.catalog.packages[target].icon_source.as_deref(),
            Some("https://example.com/cached.png")
        );
        assert!(s.icon_loader.refresh.is_empty());
        assert!(s.icon_loader.attempted.is_empty());
    }
    #[test]
    fn cached_remote_images_follow_source_identity_and_never_replace_native_icons() {
        let mut s = State::demo();
        let plan = Plan {
            icon_url: Some("https://example.com/product.png".into()),
            homepage: None,
        };
        let local = s.icons.rgba(1, 1, vec![255, 0, 0, 255]);
        local.wait().unwrap();
        let remote = s.icons.rgba(1, 1, vec![0, 255, 0, 255]);
        remote.wait().unwrap();
        for p in &mut s.catalog.packages {
            p.icon_plan = None;
        }
        s.catalog.packages[0].icon_plan = Some(plan.clone());
        s.catalog.packages[1].icon_plan = Some(plan.clone());
        s.catalog.packages[1].icon = Some(local.clone());
        s.icon_loader.loaded.insert(
            plan,
            (remote.clone(), "https://example.com/product.png".into()),
        );
        assert!(
            !s.poll_remote_icons(),
            "headless/test networking starts disabled"
        );
        s.icon_loader.enabled = true;
        assert!(s.poll_remote_icons());
        assert_eq!(
            s.catalog.packages[0].icon.as_ref().unwrap().id(),
            remote.id()
        );
        assert_eq!(
            s.catalog.packages[1].icon.as_ref().unwrap().id(),
            local.id()
        );
        assert!(s.catalog.packages[2].icon.is_none());
        assert_eq!(
            s.catalog.packages[0].icon_source.as_deref(),
            Some("https://example.com/product.png")
        );
        assert!(!s.poll_remote_icons());
        assert!(s.icon_loader.workers.iter().all(|w| !w.is_pending()));
        let plan = s.catalog.packages[0].icon_plan.clone().unwrap();
        s.icon_loader.loaded.insert(
            plan,
            (remote, "https://example.com/updated-source.png".into()),
        );
        assert!(s.poll_remote_icons());
        assert_eq!(
            s.catalog.packages[0].icon_source.as_deref(),
            Some("https://example.com/updated-source.png")
        );
    }
}
