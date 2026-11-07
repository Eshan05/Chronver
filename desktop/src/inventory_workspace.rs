use crate::{
    model::{Package, State},
    sidebar_filters::Update,
    sidebar_info,
    upgrade_preview::Plan,
};
use releasescout_engine::inventory::{InstalledPackage, Manager, Scan};
use rune::{Latest, Tx};
use rune_kit::shell::PaneState;
use std::{
    collections::{BTreeSet, HashMap},
    io::Write,
    path::PathBuf,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Column {
    Name,
    Id,
    Installed,
    Candidate,
    Status,
    Source,
    Scope,
}
impl Column {
    pub const OPTIONAL: [Self; 6] = [
        Self::Id,
        Self::Installed,
        Self::Candidate,
        Self::Status,
        Self::Source,
        Self::Scope,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Package",
            Self::Id => "Package ID",
            Self::Installed => "Installed",
            Self::Candidate => "Candidate",
            Self::Status => "Status",
            Self::Source => "Source",
            Self::Scope => "Scope",
        }
    }
    pub fn width(self) -> f32 {
        match self {
            Self::Name => 160.,
            Self::Id => 160.,
            Self::Installed | Self::Candidate => 76.,
            Self::Status => 112.,
            Self::Source => 144.,
            Self::Scope => 120.,
        }
    }
}
pub struct Review {
    pub items: Vec<ReviewItem>,
}
pub struct ReviewItem {
    pub name: String,
    pub provider: Manager,
    pub id: String,
    pub command: Result<String, String>,
}
pub struct Workspace {
    pub context: Option<crate::inventory_menu::Open>,
    pub pane: PaneState,
    pub hidden_providers: BTreeSet<String>,
    pub hidden_sources: BTreeSet<(String, String)>,
    pub expanded_providers: BTreeSet<String>,
    pub collapsed_groups: BTreeSet<String>,
    pub selected: BTreeSet<String>,
    pub sort: Column,
    pub descending: bool,
    pub columns: BTreeSet<Column>,
    pub compact: bool,
    pub columns_open: bool,
    pub review: Option<Review>,
    pub export: Latest<Result<PathBuf, String>>,
    pub export_busy: bool,
    pub export_error: Option<String>,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            context: None,
            pane: PaneState::new(240.).range(210., 300.),
            hidden_providers: Default::default(),
            hidden_sources: Default::default(),
            expanded_providers: Default::default(),
            collapsed_groups: Default::default(),
            selected: Default::default(),
            sort: Column::Name,
            descending: false,
            columns: [
                Column::Installed,
                Column::Candidate,
                Column::Status,
                Column::Source,
            ]
            .into_iter()
            .collect(),
            compact: true,
            columns_open: false,
            review: None,
            export: Latest::default(),
            export_busy: false,
            export_error: None,
        }
    }
}
#[derive(Clone)]
pub struct Record<'a> {
    pub scan: &'a Scan,
    pub package: &'a InstalledPackage,
    pub index: usize,
    pub key: String,
    pub name: String,
    pub linked: Option<&'a Package>,
    pub linked_index: Option<usize>,
}
impl Record<'_> {
    pub fn group_key(&self) -> &str {
        self.linked
            .and_then(|p| p.group_key.as_deref())
            .unwrap_or(&self.key)
    }
}
pub struct Group {
    pub key: String,
    pub members: Vec<usize>,
}
// Preserve the first matching row's sort position, without splitting a product across pages.
pub fn groups(rows: &[Record<'_>]) -> Vec<Group> {
    let mut positions: HashMap<&str, usize> = HashMap::new();
    let mut groups: Vec<Group> = Vec::new();
    for (index, record) in rows.iter().enumerate() {
        let key = record.group_key();
        if let Some(&position) = positions.get(key) {
            groups[position].members.push(index);
        } else {
            positions.insert(key, groups.len());
            groups.push(Group {
                key: key.into(),
                members: vec![index],
            });
        }
    }
    groups
}
pub fn scans(s: &State) -> Vec<&Scan> {
    let mut scans: Vec<_> = s.inventory.scans.iter().collect();
    if let Some(scan) = s
        .inventory
        .result
        .as_ref()
        .filter(|scan| !scans.iter().any(|other| other.manager == scan.manager))
    {
        scans.push(scan);
    }
    scans
}
pub fn records(s: &State) -> Vec<Record<'_>> {
    let linked: HashMap<_, _> = s
        .catalog
        .packages
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.observation.as_ref().map(|o| (o.check_key.as_str(), (i, p))))
        .collect();
    scans(s)
        .into_iter()
        .flat_map(|scan| {
            let linked = &linked;
            scan.packages
                .iter()
                .enumerate()
                .map(move |(index, p)| {
                    let key = sidebar_info::check_key(p);
                    let binding = linked.get(key.as_str()).copied();
                    let linked = binding.map(|(_, p)| p);
                    Record {
                        scan,
                        package: p,
                        index,
                        name: linked.map_or_else(
                            || crate::display_names::software(&p.name),
                            |p| s.display_name(p).into(),
                        ),
                        linked,
                        linked_index: binding.map(|(i, _)| i),
                        key,
                    }
                })
        })
        .collect()
}
pub fn source_visible(s: &State, p: &InstalledPackage) -> bool {
    let w = &s.inventory.workspace;
    !w.hidden_providers.contains(p.manager.id())
        && (w.hidden_sources.is_empty()
            || !w
                .hidden_sources
                .contains(&(p.manager.id().into(), p.source.clone())))
}
pub fn filtered(s: &State) -> Vec<Record<'_>> {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum SortKey<V> {
        Text(usize, String),
        Version(V),
        Missing,
    }

    let w = &s.inventory.workspace;
    let query = s.inventory.filter.text.trim().to_lowercase();
    let mut rows: Vec<_> = records(s)
        .into_iter()
        .filter(|r| {
            source_visible(s, r.package)
                && (s.inventory.status_filter == Update::Any
                    || crate::inventory_view::assessment(r.package, r.scan).update
                        == s.inventory.status_filter)
                && (query.is_empty()
                    || format!(
                        "{} {} {} {} {}",
                        r.name, r.package.name, r.package.id, r.package.source, r.package.scope
                    )
                    .to_lowercase()
                    .contains(&query))
        })
        .map(|r| {
            let key = match w.sort {
                Column::Name => SortKey::Text(0, r.name.to_lowercase()),
                Column::Id => SortKey::Text(0, r.package.id.to_lowercase()),
                Column::Installed | Column::Candidate => {
                    let version = if w.sort == Column::Installed {
                        r.package.installed.as_deref()
                    } else {
                        r.package.available.as_deref()
                    };
                    version
                        .filter(|v| !v.trim().is_empty())
                        .map_or(SortKey::Missing, |v| {
                            SortKey::Version(releasescout_engine::version::inventory_sort_key(v))
                        })
                }
                Column::Status => {
                    let status = crate::inventory_view::assessment(r.package, r.scan);
                    SortKey::Text(status.update as usize, status.label().into())
                }
                Column::Source => {
                    SortKey::Text(0, format!("{} / {}", r.package.manager.id(), r.package.source))
                }
                Column::Scope => SortKey::Text(0, r.package.scope.clone()),
            };
            (r, key)
        })
        .collect();
    rows.sort_by(|(a, left), (b, right)| {
        let order = match (left, right) {
            (SortKey::Missing, _) | (_, SortKey::Missing) => left.cmp(right),
            _ if w.descending => right.cmp(left),
            _ => left.cmp(right),
        };
        order
            .then_with(|| a.package.manager.id().cmp(b.package.manager.id()))
            .then_with(|| a.package.source.cmp(&b.package.source))
            .then_with(|| a.key.cmp(&b.key))
    });
    rows.into_iter().map(|(record, _)| record).collect()
}
pub fn selected(s: &State) -> Vec<Record<'_>> {
    records(s)
        .into_iter()
        .filter(|r| s.inventory.workspace.selected.contains(&r.key))
        .collect()
}
pub fn export_json(rows: &[Record<'_>]) -> Result<String, serde_json::Error> {
    let observations:Vec<_>=rows.iter().map(|r|serde_json::json!({
        "name":r.name,"observed_at":r.scan.checked_at,
        "candidate_check_requested":r.scan.update_check_requested,
        "candidate_checked_at":r.scan.update_check_requested.then(||r.scan.candidate_time()),
        "scan_update_check_succeeded":r.scan.update_check_succeeded,"scan_warnings":r.scan.warnings,
        "update_status":crate::inventory_view::assessment(r.package,r.scan).label(),"record":r.package
    })).collect();
    serde_json::to_string_pretty(
        &serde_json::json!({"schema_version":1,"exported_at":chrono::Utc::now().to_rfc3339(),"observations":observations}),
    )
}
pub fn write_export(path: &std::path::Path, body: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("Export destination needs a parent directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}
impl State {
    pub fn toggle_inventory_group_selection(&mut self, group: &str, tx: &mut Tx) {
        let keys: Vec<_> = filtered(self)
            .into_iter()
            .filter(|r| r.group_key() == group)
            .map(|r| r.key)
            .collect();
        let selected = &mut self.inventory.workspace.selected;
        let all = keys.iter().all(|key| selected.contains(key));
        for key in keys {
            if all {
                selected.remove(&key);
            } else {
                selected.insert(key);
            }
        }
        tx.instant();
    }
    pub fn reset_inventory_rows(&mut self, tx: &mut Tx) {
        self.inventory.workspace.context = None;
        self.inventory.details = None;
        tx.scroll_to_instant("inventory-scroll-area/scroll", 0., 0.);
        tx.instant();
    }
    pub fn toggle_inventory_source(&mut self, manager: Manager, source: &str, tx: &mut Tx) {
        let sources: Vec<_> = scans(self)
            .into_iter()
            .filter(|scan| scan.manager == manager)
            .flat_map(|scan| {
                scan.packages
                    .iter()
                    .map(|p| (manager.id().into(), p.source.clone()))
            })
            .collect();
        let w = &mut self.inventory.workspace;
        let key = (manager.id().into(), source.to_owned());
        if w.hidden_providers.remove(manager.id()) {
            w.hidden_sources.extend(sources);
            w.hidden_sources.remove(&key);
        } else if !w.hidden_sources.remove(&key) {
            w.hidden_sources.insert(key);
        }
        self.reset_inventory_rows(tx);
    }
    pub fn sort_inventory(&mut self, column: Column, tx: &mut Tx) {
        let w = &mut self.inventory.workspace;
        if w.sort == column {
            w.descending = !w.descending;
        } else {
            w.sort = column;
            w.descending = false;
        }
        self.reset_inventory_rows(tx);
    }
    pub fn use_inventory_key(&mut self, key: &str) {
        let found = records(self)
            .iter()
            .find(|r| r.key == key)
            .map(|r| (r.package.manager, r.index));
        if let Some((manager, index)) = found {
            self.inventory.manager = Manager::ALL.iter().position(|m| *m == manager).unwrap();
            self.use_inventory_range(index);
        } else {
            self.status = "That observation is no longer in the saved inventory".into();
        }
    }
    pub fn review_inventory_selection(&mut self) {
        self.inventory.workspace.review = Some(review_rows(selected(self)));
    }
    pub fn export_inventory(&mut self, tx: &mut Tx) {
        if self.inventory.workspace.export_busy {
            return;
        }
        let selected = selected(self);
        let rows = if selected.is_empty() {
            filtered(self)
        } else {
            selected
        };
        if rows.is_empty() {
            return;
        }
        let body = match export_json(&rows) {
            Ok(body) => body,
            Err(e) => {
                self.inventory.workspace.export_error = Some(e.to_string());
                return;
            }
        };
        self.inventory.workspace.export_busy = true;
        self.inventory.workspace.export_error = None;
        tx.save_file(
            rune::FileDialogOptions {
                title: "Export inventory".into(),
                file_name: Some("chronver-inventory.json".into()),
                ..Default::default()
            }
            .filter("JSON", &["json"]),
            move |s: &mut State, result, _| match result {
                Ok(Some(path)) => s
                    .inventory
                    .workspace
                    .export
                    .request((path, body), |(path, body)| {
                        write_export(&path, &body).map(|()| path)
                    }),
                Ok(None) => s.inventory.workspace.export_busy = false,
                Err(e) => {
                    s.inventory.workspace.export_busy = false;
                    s.inventory.workspace.export_error = Some(e.to_string());
                }
            },
        );
    }
    pub fn poll_inventory_export(&mut self) -> bool {
        let Some(result) = self.inventory.workspace.export.take() else {
            return false;
        };
        self.inventory.workspace.export_busy = false;
        match result {
            Ok(path) => self.status = format!("Inventory exported to {}", path.display()),
            Err(e) => self.inventory.workspace.export_error = Some(e),
        };
        true
    }
}

pub fn review_rows(rows: Vec<Record<'_>>) -> Review {
    let items = rows
        .into_iter()
        .map(|r| {
            let command = if crate::inventory_view::assessment(r.package, r.scan).update
                != Update::Available
            {
                Err("No provider-confirmed update candidate".into())
            } else if r.package.pinned {
                Err("Pinned observation; no upgrade command prepared".into())
            } else {
                Plan::new(r.package)
                    .ok_or_else(|| "This provider or identity has no safe command adapter".into())
                    .and_then(|p| {
                        p.command(r.package.available.as_deref().unwrap_or(""))
                            .map_err(str::to_owned)
                    })
            };
            ReviewItem {
                name: r.name,
                provider: r.package.manager,
                id: r.package.id.clone(),
                command,
            }
        })
        .collect();
    Review { items }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scan(manager: Manager, source: &str, id: &str, installed: Option<&str>) -> Scan {
        serde_json::from_value(serde_json::json!({
            "manager":manager,"checked_at":"2026-11-15T00:00:00Z","update_check_requested":true,"update_check_succeeded":true,"warnings":[],
            "packages":[{"manager":manager,"id":id,"name":"Shared Tool","installed":installed,"available":"2.0.0",
                "candidate_check_succeeded":true,"source":source,"scope":"user","root":format!("C:/fixture/{source}"),"pinned":false,"install_origin_known":false}]
        })).unwrap()
    }
    fn state() -> State {
        let mut s = State::new();
        s.accept_scan(scan(Manager::Scoop, "main", "tool", Some("1.0.0")));
        s.accept_scan(scan(
            Manager::Chocolatey,
            "community",
            "tool",
            Some("1.0.0"),
        ));
        s
    }
    #[test]
    fn combined_rows_keep_provider_identity_and_do_not_duplicate_the_result_snapshot() {
        let mut s = state();
        assert_eq!(records(&s).len(), 2);
        let keys: BTreeSet<_> = records(&s).into_iter().map(|r| r.key).collect();
        assert_eq!(keys.len(), 2);
        s.inventory.result = Some(s.inventory.scans[0].clone());
        assert_eq!(records(&s).len(), 2);
        s.inventory.workspace.selected = keys;
        assert_eq!(selected(&s).len(), 2);
        s.inventory
            .workspace
            .hidden_providers
            .insert("scoop".into());
        assert_eq!(filtered(&s).len(), 1);
        assert_eq!(
            selected(&s).len(),
            2,
            "Hidden selections must remain explicit, not silently disappear"
        );
    }
    #[test]
    fn tree_grouping_requires_verified_identity_and_keeps_all_members() {
        let mut s = state();
        assert_eq!(
            groups(&filtered(&s)).len(),
            2,
            "Equal names do not prove identity"
        );
        let mut editor = scan(
            Manager::VscodeInsiders,
            "marketplace",
            "publisher.extension",
            Some("1.0.0"),
        );
        editor.packages = (0..60)
            .map(|i| {
                let mut p = editor.packages[0].clone();
                p.scope = format!("profile: Profile {i}");
                p.extension = Some(releasescout_engine::extensions::Installed {
                    id: p.id.clone(),
                    profile_id: Some(format!("profile-{i}")),
                    profile_name: Some(format!("Profile {i}")),
                    ..Default::default()
                });
                p
            })
            .collect();
        s.accept_scan(editor);
        let rows = filtered(&s);
        let groups = groups(&rows);
        assert_eq!(groups.len(), 3);
        let group = groups.iter().find(|g| g.members.len() == 60).unwrap();
        let key = group.key.clone();
        assert_eq!(
            group
                .members
                .iter()
                .map(|&i| &rows[i].key)
                .collect::<BTreeSet<_>>()
                .len(),
            60
        );
        s.toggle_inventory_group_selection(&key, &mut Tx::default());
        assert_eq!(selected(&s).len(), 60);
        s.inventory
            .workspace
            .hidden_providers
            .insert(Manager::VscodeInsiders.id().into());
        s.toggle_inventory_group_selection(&key, &mut Tx::default());
        assert_eq!(
            selected(&s).len(),
            60,
            "A hidden group must not deselect observations"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&export_json(&selected(&s)).unwrap())
                .unwrap()["observations"]
                .as_array()
                .unwrap()
                .len(),
            60
        );
    }
    #[test]
    fn selecting_one_source_under_a_hidden_provider_enables_only_that_source() {
        let mut s = state();
        let mut scoop = s
            .inventory
            .scans
            .iter()
            .find(|s| s.manager == Manager::Scoop)
            .unwrap()
            .clone();
        let mut extra = scoop.packages[0].clone();
        extra.id = "extra".into();
        extra.source = "extras".into();
        scoop.packages.push(extra);
        s.accept_scan(scoop);
        s.inventory
            .workspace
            .hidden_providers
            .insert("scoop".into());
        s.toggle_inventory_source(Manager::Scoop, "main", &mut Tx::default());
        let rows = filtered(&s);
        assert_eq!(rows.len(), 2);
        assert!(!rows.iter().any(|r| r.package.source == "extras"));
        assert!(
            rows.iter()
                .any(|r| r.package.manager == Manager::Scoop && r.package.source == "main")
        );
    }
    #[test]
    fn version_sort_is_numeric_and_missing_versions_stay_last() {
        let mut s = state();
        s.inventory.scans[0].packages[0].installed = Some("1.9.0".into());
        s.inventory.scans[1].packages[0].installed = Some("1.10.0".into());
        let mut missing = scan(Manager::Npm, "npm", "missing", None);
        missing.packages[0].available = None;
        s.accept_scan(missing);
        s.inventory.workspace.sort = Column::Installed;
        assert_eq!(
            filtered(&s)
                .iter()
                .map(|r| r.package.installed.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("1.9.0"), Some("1.10.0"), None]
        );
        s.inventory.workspace.descending = true;
        assert_eq!(
            filtered(&s)
                .iter()
                .map(|r| r.package.installed.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("1.10.0"), Some("1.9.0"), None]
        );
    }
    #[test]
    fn rescans_prune_missing_selections_without_touching_other_providers() {
        let mut s = state();
        s.inventory.workspace.selected = records(&s).into_iter().map(|r| r.key).collect();
        s.accept_scan(scan(Manager::Scoop, "main", "replacement", Some("1.0.0")));
        let selected = selected(&s);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].package.manager, Manager::Chocolatey);
    }
    #[test]
    fn upgrade_review_preserves_records_and_explains_skipped_items() {
        let mut s = state();
        s.inventory.scans[0].packages[0].pinned = true;
        s.accept_scan(scan(Manager::Npm, "npm", "unsafe;command", Some("1.0.0")));
        s.inventory.workspace.selected = records(&s).into_iter().map(|r| r.key).collect();
        let before = serde_json::to_vec(&s.inventory.scans).unwrap();
        s.review_inventory_selection();
        let review = s.inventory.workspace.review.as_ref().unwrap();
        assert_eq!(review.items.len(), 3);
        assert_eq!(review.items.iter().filter(|i| i.command.is_ok()).count(), 1);
        assert!(review.items.iter().any(|i| {
            i.command
                .as_ref()
                .err()
                .is_some_and(|e| e.contains("Pinned"))
        }));
        assert_eq!(serde_json::to_vec(&s.inventory.scans).unwrap(), before);
        assert!(!s.inventory.work.is_pending() && !s.live.work.is_pending());
    }
    #[test]
    fn exports_are_structured_and_never_invent_check_timestamps() {
        let mut s = state();
        s.inventory.scans[0].update_check_requested = false;
        let body = export_json(&records(&s)).unwrap();
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["observations"].as_array().unwrap().len(), 2);
        assert!(json["observations"][0]["candidate_checked_at"].is_null());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.json");
        write_export(&path, &body).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        write_export(&path, "replacement").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
        assert!(write_export(&dir.path().join("missing/inventory.json"), &body).is_err());
    }
}
