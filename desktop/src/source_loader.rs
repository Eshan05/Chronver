use crate::model::State;
use releasescout_engine::{
    http::CacheOptions,
    source_discovery::{Discovery, Hints},
};
use rune::{Latest, TextEdit};
use std::collections::{HashMap, HashSet};

pub fn scene(s: &mut State, found: bool) {
    let scan = releasescout_engine::inventory::parse(releasescout_engine::inventory::Manager::Npm, false,
        &serde_json::json!({"root":"C:/global","installed":{"dependencies":{"harbor-cli":{"version":"1.2.0"}}},"metadata":{"harbor-cli":{"homepage":"https://example.com/harbor","repository":"https://github.com/example/harbor"}}}).to_string()).unwrap();
    s.accept_scan(scan);
    let hints = s.selected().unwrap().source_hints.clone().unwrap();
    if found {
        let source = releasescout_engine::source_discovery::document(
            "https://example.com/harbor/CHANGELOG.md",
            false,
        );
        s.source_loader.results.insert(
            hints,
            Discovery {
                streams: vec![releasescout_engine::Stream {
                    id: "version-notes".into(),
                    scope: "Metadata-discovered version notes".into(),
                    basis: releasescout_engine::Basis::Version,
                    scheme: releasescout_engine::Scheme::Semver,
                    channel: releasescout_engine::Channel::Stable,
                    caveats: "Automatically discovered source; coverage unverified.".into(),
                    sources: vec![source],
                }],
                attempts: vec![],
            },
        );
        s.apply_discovered_profiles();
    } else {
        s.source_loader.active = Some(hints);
    }
}

#[derive(Default)]
pub struct Loader {
    pub enabled: bool,
    pub work: Latest<(Hints, Discovery)>,
    active: Option<Hints>,
    attempted: HashSet<Hints>,
    results: HashMap<Hints, Discovery>,
    refresh: bool,
}

impl State {
    pub fn source_status(&self) -> String {
        self.selected()
            .map(|p| self.source_status_for(p))
            .unwrap_or_else(|| "No software selected".into())
    }
    pub fn source_status_for(&self, p: &crate::model::Package) -> String {
        let hints = p.source_hints.as_ref();
        let Some(hints) = hints.filter(|h| h.has_hints()) else {
            return "No release-note hints in installed metadata".into();
        };
        if let Some(result) = self.source_loader.results.get(hints) {
            return format!(
                "No usable release-note source found / {} checks recorded",
                result.attempts.len()
            );
        }
        if self.source_loader.active.as_ref() == Some(hints) {
            "Discovering release-note sources...".into()
        } else {
            "Release-note source lookup queued".into()
        }
    }

    pub fn retry_source(&mut self) {
        let Some(hints) = self.selected().and_then(|p| p.source_hints.clone()) else {
            return;
        };
        self.source_loader.attempted.remove(&hints);
        self.source_loader.results.remove(&hints);
        self.source_loader.refresh = true;
        self.poll_source_discovery();
    }

    pub fn apply_discovered_profiles(&mut self) -> bool {
        let mut changed = false;
        for p in &mut self.catalog.packages {
            let Some(hints) = &p.source_hints else {
                continue;
            };
            let Some(result) = self
                .source_loader
                .results
                .get(hints)
                .filter(|r| !r.streams.is_empty())
            else {
                continue;
            };
            let Some(profile) = self.live.catalog.packages.iter_mut().find(|x| {
                x.id == p.id
                    && x.streams.is_empty()
                    && matches!(x.tracking.as_str(), "track" | "low_priority")
            }) else {
                continue;
            };
            profile.streams = result.streams.clone();
            profile.identity = "metadata_discovered".into();
            profile.caveats = "Source discovered from installed metadata. Product/component coverage and update applicability are not reviewed.".into();
            if p.repository.is_none() {
                p.repository =
                    profile.streams.iter().flat_map(|s| &s.sources).find_map(
                        |source| match source {
                            releasescout_engine::Source::Github { owner, repo, .. } => {
                                Some(format!("https://github.com/{owner}/{repo}"))
                            }
                            releasescout_engine::Source::Document { url, .. }
                            | releasescout_engine::Source::Feed { url } => Some(url.clone()),
                            _ => None,
                        },
                    );
            }
            if let Some(q) = self.live.sessions.get_mut(&p.id) {
                q.component = 0;
                q.channel = 0;
                q.range = false;
                q.inventory_range = false;
                q.from = TextEdit::new("");
                q.to = TextEdit::new("");
                q.error = None;
                q.next = None;
                q.report = None;
            }
            changed = true;
        }
        if changed {
            self.note_prefetch.refresh();
        }
        changed
    }

    pub fn poll_source_discovery(&mut self) -> bool {
        if !self.source_loader.enabled {
            return false;
        }
        let mut changed = false;
        if let Some((hints, result)) = self.source_loader.work.take() {
            self.source_loader.results.insert(hints, result);
            self.source_loader.active = None;
            changed = true;
        }
        changed |= self.apply_discovered_profiles();
        if changed && crate::sidebar_filters::count(self) > 0 {
            self.refilter();
        }
        if !self.source_loader.work.is_pending() {
            let hints = self
                .selected()
                .filter(|p| p.extension.is_none())
                .filter(|p| {
                    self.live.catalog.packages.iter().any(|x| {
                        x.id == p.id
                            && x.streams.is_empty()
                            && matches!(x.tracking.as_str(), "track" | "low_priority")
                    })
                })
                .and_then(|p| p.source_hints.as_ref())
                .filter(|h| h.has_hints() && !self.source_loader.attempted.contains(*h))
                .cloned();
            if let Some(hints) = hints {
                self.source_loader.attempted.insert(hints.clone());
                self.source_loader.active = Some(hints.clone());
                let options = CacheOptions {
                    directory: crate::live::cache_directory(),
                    public_only: true,
                    offline: self
                        .selected()
                        .and_then(|p| self.live.sessions.get(&p.id))
                        .is_some_and(|q| q.cache == 2),
                    refresh: std::mem::take(&mut self.source_loader.refresh),
                    github_token: std::env::var("GITHUB_TOKEN").ok(),
                    ..Default::default()
                };
                self.source_loader.work.request(hints, move |hints| {
                    let result = releasescout_engine::source_discovery::cached(options, &hints);
                    (hints, result)
                });
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use releasescout_engine::{
        Basis, Channel, Scheme, Source, Stream,
        inventory::{self, Manager},
    };
    use serde_json::json;

    fn detected() -> State {
        let mut s = State::new();
        let scan=inventory::parse(Manager::Npm,false,&json!({"root":"C:/global","installed":{"dependencies":{"portable-tool":{"version":"1.0.0"}}},"metadata":{"portable-tool":{"homepage":"https://example.com","repository":"https://github.com/acme/tool"}}}).to_string()).unwrap();
        s.accept_scan(scan);
        s
    }
    fn found() -> Discovery {
        Discovery {
            streams: vec![Stream {
                id: "dated-updates".into(),
                scope: "Declared dated notes".into(),
                basis: Basis::Date,
                scheme: Scheme::Semver,
                channel: Channel::Stable,
                caveats: "Discovered".into(),
                sources: vec![Source::Feed {
                    url: "https://example.com/feed.xml".into(),
                }],
            }],
            attempts: vec![],
        }
    }
    #[test]
    fn discovery_is_disabled_in_tests_and_binds_only_matching_metadata() {
        let mut s = detected();
        let p = s.selected().unwrap();
        let id = p.id.clone();
        let hints = p.source_hints.clone().unwrap();
        assert!(hints.has_hints());
        assert!(!s.poll_source_discovery());
        assert!(!s.source_loader.work.is_pending());
        let mut stale = hints.clone();
        stale.homepage = Some("https://old.example.com".into());
        s.source_loader.results.insert(stale, found());
        assert!(!s.apply_discovered_profiles());
        assert!(
            s.live
                .catalog
                .packages
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .streams
                .is_empty()
        );
        s.source_loader.results.insert(hints, found());
        assert!(s.apply_discovered_profiles());
        let p = s.live.catalog.packages.iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.identity, "metadata_discovered");
        assert_eq!(p.streams[0].basis, Basis::Date);
        let q = &s.live.sessions[&id];
        assert!(!q.range);
        assert!(q.from.text.is_empty());
        assert!(q.to.text.is_empty());
        s.live.sessions.get_mut(&id).unwrap().from = TextEdit::new("2026-10-01");
        s.rebuild_detected();
        assert_eq!(s.live.sessions[&id].from.text, "2026-10-01");
        assert!(
            !s.live
                .catalog
                .packages
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .streams
                .is_empty()
        );
        s.inventory.scans[0].packages[0].homepage = Some("https://changed.example.com".into());
        s.rebuild_detected();
        assert!(
            s.live
                .catalog
                .packages
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .streams
                .is_empty()
        );
    }
    #[test]
    fn reviewed_profiles_win_over_auto_discovery() {
        let mut s = detected();
        let id = s.selected().unwrap().id.clone();
        let hints = s.selected().unwrap().source_hints.clone().unwrap();
        let p = s
            .live
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap();
        p.streams = found().streams;
        p.identity = "confirmed".into();
        p.streams[0].id = "reviewed".into();
        s.source_loader.results.insert(hints, found());
        assert!(!s.apply_discovered_profiles());
        assert_eq!(
            s.live
                .catalog
                .packages
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .streams[0]
                .id,
            "reviewed"
        );
    }
    #[test]
    fn failed_lookup_is_distinct_from_unconfigured_profile() {
        let mut s = detected();
        let hints = s.selected().unwrap().source_hints.clone().unwrap();
        s.source_loader.results.insert(
            hints,
            Discovery {
                attempts: vec!["Source returned no authored notes".into()],
                ..Default::default()
            },
        );
        assert!(
            s.source_status()
                .starts_with("No usable release-note source found")
        );
    }
    #[test]
    fn verified_receipt_peer_can_supply_a_reviewed_profile_but_names_cannot() {
        let mut s = State::new();
        let receipt = "ARP\\User\\X64\\portable-receipt";
        s.accept_scan(inventory::parse(Manager::Local,false,&json!({"installed":[{"Id":receipt,"Name":"GitHub CLI","Version":"2.0.0","Scope":"User"},{"Id":"ARP\\User\\X64\\different","Name":"GitHub CLI","Version":"2.0.0","Scope":"User"}]}).to_string()).unwrap());
        s.accept_scan(inventory::parse(Manager::Winget,false,&json!({"installed":{"Sources":[{"SourceDetails":{"Name":"winget"},"Packages":[{"PackageIdentifier":"GitHub.cli","Version":"2.0.0"}]}]},"metadata":{"winget/GitHub.cli":{"registrationId":receipt}}}).to_string()).unwrap());
        let local: Vec<_> = s
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected && p.manager.as_deref() == Some("Local PC"))
            .collect();
        assert_eq!(local.len(), 2);
        for p in local {
            let profile = s
                .live
                .catalog
                .packages
                .iter()
                .find(|q| q.id == p.id)
                .unwrap();
            if p.observation_label.contains(receipt) {
                assert!(!profile.streams.is_empty());
                assert_eq!(p.reference_id.as_deref(), Some("gh"));
            } else {
                assert!(profile.streams.is_empty());
            }
        }
    }
}
