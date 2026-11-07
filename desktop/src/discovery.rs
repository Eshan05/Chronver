use crate::{
    live::Query,
    model::{Dataset, EntryKind, LibrarySection, Package, State},
};
use chrono::{DateTime, Utc};
use releasescout_engine::{
    Basis, Channel,
    inventory::{Manager, ProcessRunner, Scan},
    inventory_refresh,
    matching,
};
use rune::TextEdit;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    hash::{Hash, Hasher},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

fn snapshot_path() -> PathBuf {
    crate::live::cache_directory().with_file_name("inventory.json")
}

type ScanAttempts = BTreeMap<String, DateTime<Utc>>;

fn attempts_path() -> PathBuf {
    snapshot_path().with_file_name("inventory-attempts.json")
}

fn load_attempts(path: &Path) -> io::Result<ScanAttempts> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(ScanAttempts::new()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "scan schedule exceeds 4 KiB",
        ));
    }
    let mut attempts: ScanAttempts = serde_json::from_slice(&bytes)?;
    attempts.retain(|key, _| Manager::ALL.iter().any(|m| m.id() == key));
    Ok(attempts)
}

fn save_attempts(path: &Path, attempts: &ScanAttempts) -> io::Result<()> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, attempts)?;
    file.flush()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn discovery_managers(
    scans: &[Scan],
    attempts: &ScanAttempts,
    force: bool,
    now: DateTime<Utc>,
) -> Vec<Manager> {
    Manager::ALL
        .into_iter()
        .filter(|&manager| {
            force
                || (attempts
                    .get(manager.id())
                    .is_none_or(|at| !inventory_refresh::recent_scan_attempt(*at, now))
                    && !scans.iter().any(|scan| {
                        inventory_refresh::reusable_snapshot(manager, scan, now)
                    }))
        })
        .collect()
}

impl State {
    pub fn poll_icons(&mut self) -> bool {
        let ready = self
            .catalog
            .packages
            .iter()
            .flat_map(|p| p.icon.iter().chain(p.custom_icon.iter()))
            .filter(|i| i.is_ready())
            .count();
        if ready == self.icons_ready {
            return false;
        }
        self.icons_ready = ready;
        true
    }
    pub fn start_discovery(&mut self) {
        self.discover_inventory(true, &attempts_path());
    }
    pub fn startup_discovery(&mut self) {
        self.discover_inventory(false, &attempts_path());
    }
    fn discover_inventory(&mut self, force: bool, schedule: &Path) {
        self.inventory.persist = true;
        self.inventory.discovery_errors.clear();
        // Headless scenes and model tests never call this native startup path.
        if self.inventory.scans.is_empty() {
            match matching::load_snapshot(&snapshot_path()) {
                Ok(scans) => {
                    self.inventory.scans = scans;
                    self.rebuild_detected();
                }
                Err(e) if snapshot_path().exists() => self
                    .inventory
                    .discovery_errors
                    .push(format!("Saved inventory unavailable: {e}")),
                Err(_) => {}
            }
        }
        self.status = "Discovering installed software...".into();
        let now = Utc::now();
        let mut attempts = load_attempts(schedule).unwrap_or_else(|error| {
            self.inventory
                .discovery_errors
                .push(format!("Scan schedule unavailable: {error}"));
            ScanAttempts::new()
        });
        let needed = discovery_managers(&self.inventory.scans, &attempts, force, now);
        let mut scheduled = false;
        for (work, manager) in self.inventory.discovery.iter().zip(Manager::ALL) {
            if !work.is_pending() && needed.contains(&manager) {
                attempts.insert(manager.id().into(), now);
                scheduled = true;
                let previous = self
                    .inventory
                    .scans
                    .iter()
                    .find(|s| s.manager == manager)
                    .cloned();
                work.request(manager, move |manager| {
                    let runner = ProcessRunner {
                        timeout: std::time::Duration::from_secs(if manager == Manager::Scoop {
                            180
                        } else {
                            60
                        }),
                        ..Default::default()
                    };
                    let result = if force {
                        if releasescout_engine::inventory_refresh::supports_candidates(manager) {
                            releasescout_engine::inventory_refresh::check(manager, &runner)
                        } else {
                            releasescout_engine::inventory::scan(manager, false, &runner)
                        }
                    } else {
                        releasescout_engine::inventory_refresh::startup(
                            manager,
                            previous.as_ref(),
                            &runner,
                        )
                    };
                    result.map_err(|e| e.to_string())
                });
            }
        }
        // Record attempts even when a provider is unavailable or the app closes mid-scan.
        if scheduled && let Err(error) = save_attempts(schedule, &attempts) {
            self.inventory
                .discovery_errors
                .push(format!("Scan schedule not saved: {error}"));
        }
        if !self
            .inventory
            .discovery
            .iter()
            .any(|work| work.is_pending())
        {
            self.status = format!("Saved inventory / {}", self.inventory_summary());
        }
    }
    pub fn poll_discovery(&mut self) -> bool {
        let results: Vec<_> = self
            .inventory
            .discovery
            .iter()
            .zip(Manager::ALL)
            .filter_map(|(work, m)| work.take().map(|r| (m, r)))
            .collect();
        let changed = !results.is_empty();
        for (manager, result) in results {
            match result {
                Ok(scan) => self.accept_scan(scan),
                Err(error) => self
                    .inventory
                    .discovery_errors
                    .push(format!("{}: {error}", manager.id())),
            }
        }
        if changed {
            let pending = self
                .inventory
                .discovery
                .iter()
                .filter(|w| w.is_pending())
                .count();
            self.status = format!(
                "{} / {} providers pending / {} unavailable",
                self.inventory_summary(),
                pending,
                self.inventory.discovery_errors.len()
            );
        }
        let saved = self.inventory.snapshot.take();
        if let Some(Err(error)) = &saved {
            self.inventory
                .discovery_errors
                .push(format!("Inventory could not be saved: {error}"));
        }
        if self.inventory.persist
            && self.inventory.snapshot_dirty
            && !self.inventory.snapshot.is_pending()
        {
            let scans = self.inventory.scans.clone();
            self.inventory.snapshot.request(scans, |scans| {
                matching::save_snapshot(&snapshot_path(), &scans).map_err(|e| e.to_string())
            });
            self.inventory.snapshot_dirty = false;
        }
        changed || saved.is_some()
    }
    pub fn accept_scan(&mut self, scan: Scan) {
        self.inventory.workspace.context = None;
        let error_prefix = format!("{}: ", scan.manager.id());
        self.inventory
            .discovery_errors
            .retain(|error| !error.starts_with(&error_prefix));
        if scan.update_check_requested {
            for record in scan.packages.iter().filter(|record| {
                record
                    .candidate_check_succeeded
                    .unwrap_or(scan.update_check_succeeded)
            }) {
                let key = crate::sidebar_info::check_key(record);
                self.activity.dirty |= crate::activity::record_check(
                    &mut self.activity.data.candidate_checks,
                    key,
                    scan.candidate_time(),
                );
            }
        }
        self.inventory.scans.retain(|s| s.manager != scan.manager);
        self.inventory.scans.push(scan);
        let keys: std::collections::BTreeSet<_> = crate::inventory_workspace::scans(self)
            .into_iter()
            .flat_map(|scan| scan.packages.iter().map(crate::sidebar_info::check_key))
            .collect();
        self.inventory
            .workspace
            .selected
            .retain(|key| keys.contains(key));
        self.rebuild_detected();
        self.status = self.inventory_summary();
        self.inventory.snapshot_dirty = true;
    }
    pub fn inventory_summary(&self) -> String {
        if self.section == LibrarySection::Packages {
            if !self.packages.watch.project_mode {
                return format!(
                    "{} packages / {} tracked",
                    self.rows.len(),
                    self.packages
                        .watch
                        .entries
                        .iter()
                        .filter(|entry| entry.ecosystem
                            == releasescout_engine::watchlist::Ecosystem::Npm)
                        .count()
                );
            }
            return format!(
                "{} packages / {} declarations / {} projects",
                self.rows.len(),
                self.catalog
                    .packages
                    .iter()
                    .filter(|p| p.dependency.is_some())
                    .count(),
                self.packages.scan.as_ref().map_or(0, |s| s.projects.len())
            );
        }
        let components = self
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected && p.component_reason.is_some())
            .count();
        if self.dataset == Dataset::Detected {
            if self.section == LibrarySection::Extensions {
                return format!("{} extension memberships", self.rows.len());
            }
            format!(
                "{} software groups / {} system/support {}",
                self.rows.len(),
                components,
                if self.show_components {
                    "included"
                } else {
                    "hidden"
                }
            )
        } else {
            "Demo data / not a machine inventory".into()
        }
    }
    pub fn rebuild_detected(&mut self) {
        let selected = self.selected().map(|p| p.id.clone());
        let discovered: HashMap<_, _> = self
            .live
            .catalog
            .packages
            .iter()
            .filter(|p| p.identity == "metadata_discovered")
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
        let mut old: HashMap<_, _> = self
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected)
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
        self.catalog
            .packages
            .retain(|p| !p.detected || p.dependency.is_some());
        self.live
            .catalog
            .packages
            .retain(|p| !p.id.starts_with("detected-"));
        let bindings = matching::bindings();
        let groups = releasescout_engine::presentation::observations(&self.inventory.scans);
        let mut group_bindings: HashMap<&str, Option<&matching::Binding>> = HashMap::new();
        for g in &groups {
            let record = &self.inventory.scans[g.scan].packages[g.package];
            if let Some(binding) = matching::resolve(record, &bindings) {
                group_bindings
                    .entry(&g.group)
                    .and_modify(|current| {
                        if current.is_some_and(|b| {
                            b.product != binding.product || b.stream != binding.stream
                        }) {
                            *current = None;
                        }
                    })
                    .or_insert(Some(binding));
            }
        }
        let mut seen = HashSet::new();
        for (si, scan) in self.inventory.scans.iter().enumerate() {
            for (pi, record) in scan.packages.iter().enumerate() {
                let presentation = groups
                    .iter()
                    .find(|g| g.scan == si && g.package == pi)
                    .unwrap();
                // Keep separate managers, sources, roots, scopes and installed versions distinct.
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (
                    record.manager.id(),
                    &record.id,
                    &record.scope,
                    &record.source,
                    &record.root,
                    &record.installed,
                )
                    .hash(&mut h);
                if let Some(profile) = record
                    .extension
                    .as_ref()
                    .and_then(|e| e.profile_id.as_ref())
                {
                    profile.hash(&mut h);
                }
                let id = format!("detected-{:016x}", h.finish());
                if !seen.insert(id.clone()) {
                    continue;
                }
                let binding = matching::resolve(record, &bindings).or_else(|| {
                    group_bindings
                        .get(presentation.group.as_str())
                        .copied()
                        .flatten()
                });
                let template = binding.and_then(|b| {
                    self.live
                        .catalog
                        .packages
                        .iter()
                        .find(|p| p.id == b.product)
                });
                let mut profile =
                    template
                        .cloned()
                        .unwrap_or_else(|| releasescout_engine::Package {
                            id: id.clone(),
                            name: record.name.clone(),
                            tracking: "track".into(),
                            identity: "manager_observed".into(),
                            alias_of: None,
                            caveats: "No release-note identity mapping for this detected package"
                                .into(),
                            reference_urls: vec![],
                            streams: vec![],
                        });
                profile.id = id.clone();
                if let Some(component) = binding.and_then(|b| b.stream.as_ref()) {
                    profile.streams.retain(|s| &s.id == component);
                }
                let source_hints = record.extension.is_none().then(|| {
                    let mut hints =
                        releasescout_engine::source_discovery::Hints::from_record(record);
                    hints.references = profile.reference_urls.clone();
                    for peer in groups.iter().filter(|g| g.group == presentation.group) {
                        let other = &self.inventory.scans[peer.scan].packages[peer.package];
                        hints.homepage = hints.homepage.or_else(|| other.homepage.clone());
                        hints.release_notes =
                            hints.release_notes.or_else(|| other.release_notes.clone());
                        if hints.repository.is_none()
                            && (other.manager == record.manager
                                || !matches!(
                                    other.manager,
                                    Manager::Npm | Manager::Pnpm | Manager::Bun
                                ))
                        {
                            hints.repository = other.repository.clone();
                            hints.directory = other.repository_directory.clone();
                        }
                    }
                    hints
                });
                if matches!(profile.tracking.as_str(), "track" | "low_priority")
                    && profile.streams.is_empty()
                    && old.get(&id).is_some_and(|p| p.source_hints == source_hints)
                    && let Some(previous) = discovered.get(&id)
                {
                    profile = previous.clone();
                }
                let name = presentation
                    .name
                    .clone()
                    .unwrap_or_else(|| profile.name.clone());
                let name = if record.manager.is_editor() {
                    name
                } else {
                    crate::display_names::software(&name)
                };
                let repository = record
                    .extension
                    .as_ref()
                    .map(|e| {
                        e.repository
                            .as_deref()
                            .and_then(|u| releasescout_engine::http::https(u).ok())
                            .map(Into::into)
                            .unwrap_or_else(|| {
                                format!(
                                    "https://marketplace.visualstudio.com/items?itemName={}",
                                    e.id
                                )
                            })
                    })
                    .or_else(|| {
                        profile
                            .streams
                            .iter()
                            .flat_map(|s| &s.sources)
                            .find_map(|s| match s {
                                releasescout_engine::Source::Github { owner, repo, .. } => {
                                    Some(format!("https://github.com/{owner}/{repo}"))
                                }
                                _ => None,
                            })
                    });
                let note = if record.available.is_some() {
                    "Candidate available".into()
                } else {
                    record
                        .candidate_note
                        .clone()
                        .unwrap_or_else(|| "Update candidate not checked or unavailable".into())
                };
                let mut p = Package {
                    id: id.clone(),
                    name,
                    description: crate::sidebar_info::clean_description(
                        record.evidence.description.as_deref(),
                    ),
                    description_fallback: crate::descriptions::lookup(
                        record,
                        binding.map(|b| b.product.as_str()),
                    ),
                    provider: Some(record.manager),
                    observation: Some(crate::sidebar_info::Observation::new(record, scan)),
                    manager: Some(if record.manager == Manager::Local {
                        "Local PC".into()
                    } else {
                        format!("Detected by {}", record.manager.id())
                    }),
                    repository,
                    installed: record.installed.clone(),
                    target: record.available.clone(),
                    reference_text: Some(if record.extension.is_some() {
                        format!(
                            "{} / scanned {}",
                            record.scope,
                            scan.checked_at.get(..10).unwrap_or(&scan.checked_at)
                        )
                    } else {
                        format!(
                            "{} / {} / {} / checked {}",
                            record.id, record.source, record.scope, scan.checked_at
                        )
                    }),
                    kind: if record.manager.is_editor() {
                        EntryKind::Extension
                    } else {
                        EntryKind::Package
                    },
                    removed: false,
                    unconfirmed: false,
                    sample_notes: false,
                    releases: vec![],
                    detected: true,
                    dependency: None,
                    watchlist: None,
                    reference_id: binding.map(|b| b.product.clone()),
                    inventory_note: Some(note),
                    group_key: Some(presentation.group.clone()),
                    component_reason: presentation.component_reason.clone(),
                    observation_label: format!(
                        "{} / {} / {} / {} / {}{}{}",
                        record.manager.id(),
                        record.id,
                        record.installed.as_deref().unwrap_or("unknown version"),
                        record.scope,
                        record.source,
                        record
                            .root
                            .as_ref()
                            .map(|r| format!(" / {r}"))
                            .unwrap_or_default(),
                        if presentation.wrapper {
                            " / wrapper"
                        } else {
                            ""
                        }
                    ),
                    custom_icon: None,
                    custom_symbol: None,
                    icon: record.icon_path.as_deref().and_then(|path| {
                        if record.evidence.has_commands == Some(true)
                            && record.evidence.has_shortcuts != Some(true)
                            && std::path::Path::new(path).extension().is_some_and(|e| {
                                e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll")
                            })
                        {
                            return None;
                        }
                        let bytes = path.as_bytes();
                        if bytes.len() < 3
                            || !bytes[0].is_ascii_alphabetic()
                            || bytes[1] != b':'
                            || !matches!(bytes[2], b'/' | b'\\')
                        {
                            return None;
                        }
                        let ext = std::path::Path::new(path)
                            .extension()?
                            .to_str()?
                            .to_ascii_lowercase();
                        match ext.as_str() {
                            "exe" | "dll" => Some(self.icons.app_icon(path, 40.0)),
                            "png" | "ico" | "jpg" | "jpeg" | "webp" => {
                                Some(self.icons.file(path, 40.0, 2.0))
                            }
                            _ => None,
                        }
                    }),
                    icon_source: record.icon_path.clone(),
                    extension: record.extension.clone(),
                    source_hints,
                    icon_plan: Some(releasescout_engine::icons::Plan {
                        icon_url: record
                            .icon_url
                            .as_deref()
                            .and_then(releasescout_engine::icons::safe_url)
                            .map(Into::into),
                        homepage: record
                            .homepage
                            .as_deref()
                            .and_then(releasescout_engine::icons::product_homepage)
                            .or_else(|| {
                                profile
                                    .reference_urls
                                    .iter()
                                    .find_map(|u| releasescout_engine::icons::product_homepage(u))
                            }),
                    }),
                    display_priority: if presentation.wrapper {
                        100
                    } else if record.manager == Manager::Local {
                        20
                    } else if record.install_origin_known {
                        0
                    } else {
                        10
                    },
                };
                if let Some(previous) = old.remove(&id) {
                    if p.icon.is_none()
                        && p.icon_plan == previous.icon_plan
                        && previous
                            .icon_source
                            .as_ref()
                            .is_some_and(|source| source.starts_with("https://"))
                    {
                        p.icon = previous.icon;
                        p.icon_source = previous.icon_source;
                    }
                    p.releases = previous.releases;
                }
                self.live
                    .sessions
                    .entry(id.clone())
                    .and_modify(|q| {
                        if q.inventory_range {
                            q.from = TextEdit::new(record.installed.as_deref().unwrap_or(""));
                            q.to = TextEdit::new(record.available.as_deref().unwrap_or(""));
                            q.range = record.available.is_some();
                        }
                    })
                    .or_insert_with(|| {
                        let version = profile
                            .streams
                            .first()
                            .is_some_and(|s| s.basis == Basis::Version);
                        Query {
                            inventory_range: version,
                            range: version && record.available.is_some(),
                            from: TextEdit::new(if version {
                                record.installed.as_deref().unwrap_or("")
                            } else {
                                ""
                            }),
                            to: TextEdit::new(if version {
                                record.available.as_deref().unwrap_or("")
                            } else {
                                ""
                            }),
                            channel: profile.streams.first().map_or(
                                usize::from(
                                    record
                                        .extension
                                        .as_ref()
                                        .is_some_and(|e| e.pre_release == Some(true)),
                                ),
                                |s| match s.channel {
                                    Channel::Stable => 0,
                                    Channel::Preview => 1,
                                    Channel::All => 2,
                                },
                            ),
                            ..Default::default()
                        }
                    });
                self.live.catalog.packages.push(profile);
                self.catalog.packages.push(p);
            }
        }
        self.apply_discovered_profiles();
        let alive: HashSet<_> = self.catalog.packages.iter().map(|p| p.id.clone()).collect();
        if self
            .live
            .active
            .as_ref()
            .is_some_and(|id| !alive.contains(id))
        {
            self.live.work.cancel();
            self.live.history.cancel();
            self.live.active = None;
        }
        self.live.sessions.retain(|id, _| alive.contains(id));
        self.normalize_extension_filter();
        self.set_dataset(Dataset::Detected);
        if let Some(id) = selected {
            self.select_package(&id);
        }
        self.note_prefetch.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use releasescout_engine::inventory::parse;
    use serde_json::json;
    #[test]
    fn reopen_reuses_providers_within_a_day_and_manual_refresh_bypasses_it() {
        let now = chrono::Utc::now();
        let dir = tempfile::tempdir().unwrap();
        let schedule = dir.path().join("attempts.json");
        let attempts = ScanAttempts::new();
        let mut s = State::new();
        s.inventory.scans = Manager::ALL
            .into_iter()
            .map(|manager| Scan {
                manager,
                checked_at: (now - chrono::Duration::hours(23)).to_rfc3339(),
                candidate_checked_at: None,
                update_check_requested: releasescout_engine::inventory_refresh::supports_candidates(
                    manager,
                ),
                update_check_succeeded: true,
                packages: vec![],
                warnings: vec![],
            })
            .collect();
        let before = serde_json::to_vec(&s.inventory.scans).unwrap();
        assert!(discovery_managers(&s.inventory.scans, &attempts, false, now).is_empty());
        assert_eq!(
            discovery_managers(&s.inventory.scans, &attempts, true, now),
            Manager::ALL
        );
        s.discover_inventory(false, &schedule);
        assert!(s.inventory.discovery.iter().all(|w| !w.is_pending()));
        assert_eq!(serde_json::to_vec(&s.inventory.scans).unwrap(), before);
        assert!(!s.inventory.snapshot_dirty);
        assert!(!schedule.exists());
        let scan = s
            .inventory
            .scans
            .iter_mut()
            .find(|s| s.manager == Manager::Scoop)
            .unwrap();
        scan.checked_at = (now - chrono::Duration::hours(24)).to_rfc3339();
        assert_eq!(
            discovery_managers(&s.inventory.scans, &attempts, false, now),
            vec![Manager::Scoop]
        );
        s.inventory.scans.retain(|s| s.manager != Manager::Local);
        let needed = discovery_managers(&s.inventory.scans, &attempts, false, now);
        assert!(needed.contains(&Manager::Local) && needed.contains(&Manager::Scoop));
    }

    #[test]
    fn saved_attempts_throttle_unavailable_providers_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("attempts.json");
        let now = Utc::now();
        let attempts = ScanAttempts::from([
            (Manager::Bun.id().into(), now - chrono::Duration::hours(23)),
            (Manager::Scoop.id().into(), now - chrono::Duration::hours(24)),
            (Manager::Local.id().into(), now + chrono::Duration::hours(1)),
        ]);
        save_attempts(&path, &attempts).unwrap();
        let reloaded = load_attempts(&path).unwrap();
        assert_eq!(reloaded, attempts);
        let needed = discovery_managers(&[], &reloaded, false, now);
        assert!(!needed.contains(&Manager::Bun));
        assert!(needed.contains(&Manager::Scoop) && needed.contains(&Manager::Local));
        assert_eq!(discovery_managers(&[], &reloaded, true, now), Manager::ALL);
    }
    fn shown_across_sections(s: &mut State) -> usize {
        let previous = s.section;
        let mut shown = 0;
        for section in [LibrarySection::Software, LibrarySection::Extensions] {
            s.set_section(section);
            shown += s.rows.iter().map(|r| r.members.len()).sum::<usize>();
        }
        s.set_section(previous);
        shown
    }
    #[test]
    #[ignore = "explicit local snapshot reconciliation check; does not scan or save"]
    fn local_snapshot_reconciliation_preserves_all_observations() {
        let scans = matching::load_snapshot(&snapshot_path()).unwrap();
        let raw = scans.iter().map(|s| s.packages.len()).sum::<usize>();
        let mut s = State::new();
        for scan in scans {
            s.accept_scan(scan);
        }
        let detected = s.catalog.packages.iter().filter(|p| p.detected).count();
        let hidden = s
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected && p.component_reason.is_some())
            .count();
        assert_eq!(detected, raw);
        assert_eq!(shown_across_sections(&mut s) + hidden, raw);
        let row_ids: HashSet<_> = s.rows.iter().map(|r| &r.id).collect();
        assert_eq!(row_ids.len(), s.rows.len());
        assert!(!s.inventory.snapshot.is_pending());
        assert!(!s.inventory.persist);
        println!("{raw} raw observations; {}", s.inventory_summary());
        s.toggle_components();
        assert_eq!(shown_across_sections(&mut s), raw);
    }
    #[test]
    #[ignore = "explicit local provider-artifact reconciliation; does not scan or save"]
    fn local_native_artifacts_reconcile_with_existing_inventory_and_icons() {
        let mut s = State::new();
        for scan in matching::load_snapshot(&snapshot_path()).unwrap() {
            s.accept_scan(scan);
        }
        for name in ["local", "appx", "winget"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../artifacts/native-{name}.json"
            ));
            s.accept_scan(serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap());
        }
        let raw = s
            .inventory
            .scans
            .iter()
            .map(|scan| scan.packages.len())
            .sum::<usize>();
        let hidden = s
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected && p.component_reason.is_some())
            .count();
        assert_eq!(shown_across_sections(&mut s) + hidden, raw);
        let icons = s
            .catalog
            .packages
            .iter()
            .filter_map(|p| p.icon.as_ref())
            .filter(|i| i.wait().is_ok())
            .count();
        assert!(icons > 0);
        assert!(!s.inventory.persist);
        println!(
            "{raw} raw records / {icons} decoded icons / {}",
            s.inventory_summary()
        );
        let local_only = s
            .rows
            .iter()
            .filter(|r| {
                r.members
                    .iter()
                    .all(|&i| s.catalog.packages[i].manager_label() == "Local PC")
            })
            .count();
        println!("{local_only} local-only visible groups");
    }
    #[test]
    #[ignore = "explicit local discovery/snapshot integration check; requires all providers"]
    fn local_startup_discovery_drives_the_main_list() {
        let mut s = State::new();
        s.start_discovery();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        loop {
            s.poll_discovery();
            if !s.inventory.discovery.iter().any(|w| w.is_pending())
                && !s.inventory.snapshot.is_pending()
                && !s.inventory.snapshot_dirty
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "discovery did not settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            s.inventory.discovery_errors.is_empty(),
            "Provider failures: {:?}",
            s.inventory.discovery_errors
        );
        assert!(s.dataset == Dataset::Detected);
        assert!(!s.visible.is_empty());
        assert!(
            s.visible
                .iter()
                .all(|i| s.catalog.packages[s.rows[*i].package].detected)
        );
        let saved = matching::load_snapshot(&snapshot_path()).unwrap();
        assert_eq!(saved.len(), s.inventory.scans.len());
        let raw = s.catalog.packages.iter().filter(|p| p.detected).count();
        let hidden = s
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected && p.component_reason.is_some())
            .count();
        let shown_observations = shown_across_sections(&mut s);
        assert_eq!(shown_observations + hidden, raw);
        println!(
            "{raw} raw observations; {hidden} hidden components; {} displayed product groups; {} successful providers",
            s.visible.len(),
            saved.len()
        );
        println!("{}", s.inventory_summary());
    }
    fn scoop(version: &str) -> Scan {
        parse(Manager::Scoop,true,&json!({"installed":{"apps":[{"Name":"gh","Version":version,"Source":"main"},{"Name":"mystery-tool","Version":"1.0","Source":"extras"}]},"updates":[{"Name":"gh","Installed Version":version,"Latest Version":"3.0.0"}]}).to_string()).unwrap()
    }
    #[test]
    fn detected_rows_drive_the_sidebar_and_exact_matches_have_ranges() {
        let mut s = State::new();
        s.accept_scan(scoop("2.0.0"));
        assert!(s.dataset == Dataset::Detected);
        assert_eq!(s.visible.len(), 2);
        let gh = s
            .catalog
            .packages
            .iter()
            .find(|p| p.detected && p.name == "GitHub CLI")
            .unwrap();
        assert_eq!(gh.installed.as_deref(), Some("2.0.0"));
        assert_eq!(gh.target.as_deref(), Some("3.0.0"));
        assert_eq!(gh.update_label(), "Candidate available");
        assert_eq!(gh.reference_id.as_deref(), Some("gh"));
        assert_eq!(gh.repository.as_deref(), Some("https://github.com/cli/cli"));
        assert!(gh.manager_label().contains("scoop"));
        let q = &s.live.sessions[&gh.id];
        assert!(q.range);
        assert_eq!(q.from.text, "2.0.0");
        assert_eq!(q.to.text, "3.0.0");
        let unknown = s
            .catalog
            .packages
            .iter()
            .find(|p| p.name == "Mystery Tool")
            .unwrap();
        assert_eq!(
            s.live.unavailable(&unknown.id).as_deref(),
            Some("Source profile not configured")
        );
        assert!(s.inventory.snapshot_dirty);
        assert!(!s.inventory.persist);
        assert!(!s.inventory.snapshot.is_pending());
        assert!(s.catalog.packages.iter().all(|p| p.detected));
    }
    #[test]
    fn refresh_replaces_only_its_manager_and_does_not_merge_observations() {
        let mut s = State::new();
        s.accept_scan(scoop("2.0.0"));
        let npm=parse(Manager::Pnpm,false,&json!({"root":"C:/fixture/global","installed":{"dependencies":{"pnpm":{"version":"12.0.0"}}},"installedExit":0}).to_string()).unwrap();
        s.accept_scan(npm);
        assert_eq!(s.visible.len(), 3);
        s.accept_scan(scoop("2.1.0"));
        assert_eq!(s.visible.len(), 3);
        assert!(
            s.catalog
                .packages
                .iter()
                .filter(|p| p.detected)
                .all(|p| p.installed.as_deref() != Some("2.0.0"))
        );
        s.accept_scan(
            parse(
                Manager::Scoop,
                false,
                &json!({"installed":{"apps":[]}}).to_string(),
            )
            .unwrap(),
        );
        assert_eq!(s.visible.len(), 1);
        assert!(s.selected().unwrap().manager_label().contains("pnpm"));
    }
    #[test]
    fn installed_only_then_candidate_check_fills_range_without_manual_copy() {
        let mut scan = scoop("2.0.0");
        scan.packages[0].available = None;
        let mut s = State::new();
        s.accept_scan(scan);
        let id = s
            .catalog
            .packages
            .iter()
            .find(|p| p.detected && p.name == "GitHub CLI")
            .unwrap()
            .id
            .clone();
        assert!(s.live.sessions[&id].to.text.is_empty());
        s.accept_scan(scoop("2.0.0"));
        assert_eq!(s.live.sessions[&id].to.text, "3.0.0");
        let mut installed_only = scoop("2.0.0");
        installed_only.packages[0].available = None;
        s.accept_scan(installed_only);
        assert!(s.live.sessions[&id].to.text.is_empty());
        let q = s.live.sessions.get_mut(&id).unwrap();
        q.inventory_range = false;
        q.to = TextEdit::new("9.0.0");
        s.accept_scan(scoop("2.0.0"));
        assert_eq!(s.live.sessions[&id].to.text, "9.0.0");
    }

    #[test]
    fn local_source_hint_flows_to_exactly_matched_inventory() {
        let mut state = State::new();
        let settings = tempfile::tempdir().unwrap();
        let notes = "https://example.com/steam/notes";
        std::fs::write(
            settings.path().join("catalog-overrides.json"),
            json!({"schema_version":1,"products":[{"id":"steam","reference_urls":[notes]}]}).to_string(),
        )
        .unwrap();
        state.live.load_local_settings(settings.path());
        assert!(state.live.settings_errors.is_empty());
        state.accept_scan(parse(Manager::Local, false, &json!({"installed":[
            {"Id":"ARP/User/X64/unfamiliar-id","Name":"Steam","Version":"9.9","Scope":"User","metadata":{"publisher":"Valve Corporation"}},
            {"Id":"ARP/User/X64/other-id","Name":"Steam","Version":"1.0","Scope":"User","metadata":{"publisher":"Unrelated vendor"}}
        ]}).to_string()).unwrap());
        let with_hint = state.catalog.packages.iter().filter(|package| {
            package.detected && package.source_hints.as_ref().is_some_and(|hints| {
                hints.references.iter().any(|url| url == notes)
            })
        }).count();
        assert_eq!(with_hint, 1);
        assert_eq!(state.rows.len(), 2, "a private preference is not a default");
    }

    #[test]
    fn explicit_tracking_exclusions_hide_rows_without_removing_inventory_or_other_publishers() {
        let mut state = State::new();
        let settings = tempfile::tempdir().unwrap();
        std::fs::write(
            settings.path().join("tracking.json"),
            r#"{"schema_version":1,"products":[{"id":"steam","tracking":"excluded"}]}"#,
        ).unwrap();
        state.live.load_local_settings(settings.path());
        state.accept_scan(parse(Manager::Local, false, &json!({"installed":[
            {"Id":"ARP/User/X64/unfamiliar-id","Name":"Steam","Version":"9.9","Scope":"User","metadata":{"publisher":"Valve Corporation"}},
            {"Id":"ARP/User/X64/other-id","Name":"Steam","Version":"1.0","Scope":"User","metadata":{"publisher":"Unrelated vendor"}}
        ]}).to_string()).unwrap());
        assert_eq!(state.inventory.scans[0].packages.len(), 2);
        assert_eq!(state.rows.len(), 1);
        state.show_components = true;
        state.set_dataset(crate::model::Dataset::Detected);
        assert_eq!(state.rows.len(), 1);
        let excluded = state
            .live
            .catalog
            .packages
            .iter()
            .filter(|p| p.tracking == "excluded" && p.id.starts_with("detected-"))
            .count();
        assert_eq!(excluded, 1);
        let streams = state
            .live
            .catalog
            .packages
            .iter()
            .find(|p| p.id == "gh")
            .unwrap()
            .streams
            .clone();
        let old = state
            .live
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.tracking == "excluded" && p.id.starts_with("detected-"))
            .unwrap();
        old.tracking = "track".into();
        old.identity = "metadata_discovered".into();
        old.streams = streams;
        state.rebuild_detected();
        assert_eq!(
            state.rows.len(),
            1,
            "cached automatic profiles cannot undo an explicit exclusion"
        );
    }
}
