use crate::model::State;
use releasescout_engine::inventory::{Manager, ProcessRunner, Scan};
use rune::{El, Latest, TextEdit};
use rune_kit::Kit;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub struct Inventory {
    pub workspace: crate::inventory_workspace::Workspace,
    pub projection: crate::inventory_view::ProjectionCache,
    pub discovery: Vec<Latest<std::result::Result<Scan, String>>>,
    pub snapshot: Latest<std::result::Result<(), String>>,
    pub persist: bool,
    pub snapshot_dirty: bool,
    pub scans: Vec<Scan>,
    pub discovery_errors: Vec<String>,
    pub open: bool,
    pub manager: usize,
    pub picker: bool,
    pub status_filter: crate::sidebar_filters::Update,
    pub details: Option<String>,
    pub notices_open: bool,
    pub loading: bool,
    pub filter: TextEdit,
    pub result: Option<Scan>,
    pub error: Option<String>,
    pub candidate_errors: std::collections::BTreeMap<String, String>,
    pub last_request: Option<(Manager, bool)>,
    pub work: Latest<std::result::Result<Scan, String>>,
    pub cancel: Arc<AtomicBool>,
}
impl Default for Inventory {
    fn default() -> Self {
        Self {
            workspace: Default::default(),
            projection: Default::default(),
            discovery: Manager::ALL.iter().map(|_| Latest::default()).collect(),
            snapshot: Latest::default(),
            persist: false,
            snapshot_dirty: false,
            scans: vec![],
            discovery_errors: vec![],
            open: false,
            manager: 0,
            picker: false,
            status_filter: Default::default(),
            details: None,
            notices_open: false,
            loading: false,
            filter: TextEdit::new(""),
            result: None,
            error: None,
            candidate_errors: Default::default(),
            last_request: None,
            work: Latest::default(),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Inventory {
    pub(crate) fn selected_scan(&self) -> Option<&Scan> {
        let manager = Manager::ALL[self.manager];
        self.scans
            .iter()
            .find(|scan| scan.manager == manager)
            .or_else(|| self.result.as_ref().filter(|scan| scan.manager == manager))
    }
}
impl State {
    pub fn start_inventory(&mut self, updates: bool) {
        self.inventory.workspace.context = None;
        self.inventory.last_request = Some((Manager::ALL[self.inventory.manager], updates));
        self.inventory.discovery[self.inventory.manager].cancel();
        self.inventory.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        self.inventory.cancel = cancel.clone();
        self.inventory.loading = true;
        self.inventory.error = None;
        let manager = Manager::ALL[self.inventory.manager];
        let previous = (!updates)
            .then(|| {
                self.inventory
                    .scans
                    .iter()
                    .find(|scan| scan.manager == manager)
                    .cloned()
            })
            .flatten();
        self.inventory
            .work
            .request((manager, updates), move |(manager, updates)| {
                let runner = ProcessRunner {
                    cancel,
                    timeout: std::time::Duration::from_secs(
                        if manager == Manager::Scoop && updates {
                            180
                        } else {
                            60
                        },
                    ),
                };
                (if updates {
                    releasescout_engine::inventory_refresh::check(manager, &runner)
                } else {
                    releasescout_engine::inventory_refresh::startup(
                        manager,
                        previous.as_ref(),
                        &runner,
                    )
                })
                .map_err(|e| e.to_string())
            });
    }
    pub fn poll_inventory(&mut self) -> bool {
        let Some(result) = self.inventory.work.take() else {
            return false;
        };
        self.inventory.loading = false;
        match result {
            Ok(scan) => {
                if scan.update_check_requested && !scan.candidate_results_reused {
                    self.inventory.candidate_errors.remove(scan.manager.id());
                }
                self.inventory.error = None;
                self.accept_scan(scan.clone());
                self.inventory.result = Some(scan);
            }
            Err(error) => {
                if let Some((manager, true)) = self.inventory.last_request.take() {
                    self.inventory
                        .candidate_errors
                        .insert(manager.id().into(), error.clone());
                }
                self.inventory.error = Some(error);
            }
        }
        true
    }
    pub fn use_inventory_range(&mut self, index: usize) {
        let Some(p) = self
            .inventory
            .selected_scan()
            .and_then(|r| r.packages.get(index))
        else {
            return;
        };
        let from = p.installed.clone().unwrap_or_default();
        let to = p.available.clone().unwrap_or_default();
        let provenance = format!("{} / {} / {}", p.manager.id(), p.source, p.id);
        let Some(id) = self.selected().map(|p| p.id.clone()) else {
            return;
        };
        let profile = self
            .live
            .catalog
            .packages
            .iter()
            .find(|p| p.id == id)
            .unwrap();
        let q = &self.live.sessions[&id];
        if profile
            .streams
            .get(q.component)
            .is_none_or(|s| s.basis != releasescout_engine::Basis::Version)
        {
            self.inventory.error = Some(
                "Select a reference with a version-based stream before using this range".into(),
            );
            return;
        }
        let q = self.engine_query();
        q.inventory_range = false;
        q.range = true;
        q.from = TextEdit::new(&from);
        q.to = TextEdit::new(&to);
        q.error = None;
        self.inventory.open = false;
        self.mark_current_notes_read();
        self.status = format!("Range copied from {provenance}; verify the product match");
    }
}
pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>, cx: &rune::Cx<State>) -> (El, (usize, usize)) {
    crate::inventory_view::view(s, k, layers, cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Scan {
        releasescout_engine::inventory::parse(Manager::Npm,true,&serde_json::json!({"root":"C:/fixture/global","installed":{"dependencies":{"fixture-tool":{"version":"1.0.0"}}},"installedExit":0,"updateExit":1,"updates":{"fixture-tool":{"current":"1.0.0","latest":"2.0.0"}}}).to_string()).unwrap()
    }
    #[test]
    fn state_starts_without_inventory_until_startup_discovery() {
        let s = State::new();
        assert!(!s.inventory.loading);
        assert!(s.inventory.result.is_none());
        assert!(!s.inventory.work.is_pending());
        assert!(s.catalog.packages.is_empty());
    }
    #[test]
    fn use_range_is_explicit_and_does_not_change_install_metadata() {
        let mut s = State::demo();
        s.select_package("gh");
        s.inventory.manager = Manager::ALL
            .iter()
            .position(|m| *m == Manager::Npm)
            .unwrap();
        s.inventory.scans.push(fixture());
        s.inventory.open = true;
        s.use_inventory_range(0);
        assert!(!s.inventory.open);
        assert_eq!(s.engine_query().from.text, "1.0.0");
        assert_eq!(s.engine_query().to.text, "2.0.0");
        assert!(s.engine_query().range);
        assert!(s.selected().unwrap().installed.is_none());
        assert!(s.selected().unwrap().target.is_none());
        assert!(!s.live.work.is_pending());
        assert!(!s.live.history.is_pending());
    }
    #[test]
    fn dated_feeds_do_not_receive_binary_versions() {
        let mut s = State::demo();
        s.select_package("notion");
        s.inventory.manager = Manager::ALL
            .iter()
            .position(|m| *m == Manager::Npm)
            .unwrap();
        s.inventory.result = Some(fixture());
        s.inventory.open = true;
        s.use_inventory_range(0);
        assert!(s.inventory.open);
        assert!(
            s.inventory
                .error
                .as_ref()
                .unwrap()
                .contains("version-based")
        );
        assert!(s.engine_query().from.text.is_empty());
    }
    #[test]
    fn background_scan_populates_sidebar_and_empty_rescan_does_not_reintroduce_demo_rows() {
        let mut s = State::new();
        s.inventory.work.request((), |_| Ok(fixture()));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !s.poll_inventory() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            s.inventory.result.as_ref().unwrap().packages[0].id,
            "fixture-tool"
        );
        assert_eq!(s.catalog.packages.len(), 1);
        assert!(s.dataset == crate::model::Dataset::Detected);
        assert_eq!(s.visible.len(), 1);
        let mut empty = fixture();
        empty.packages.clear();
        s.accept_scan(empty);
        assert!(s.dataset == crate::model::Dataset::Detected);
        assert!(s.catalog.packages.is_empty());
        assert!(s.visible.is_empty());
        assert!(s.selected().is_none());
    }

    #[test]
    fn successful_editor_retry_clears_only_its_stale_profiles_and_notice() {
        let mut s = State::new();
        let profile = r#"{"profiles":[{"name":"Work","id":"work","installed":"vendor.tool@1.0"}]}"#;
        let parse = releasescout_engine::inventory::parse;
        s.accept_scan(parse(Manager::Vscode, false, profile).unwrap());
        s.accept_scan(parse(Manager::VscodeInsiders, false, profile).unwrap());
        s.inventory.discovery_errors = vec!["vscode: old error".into(), "winget: other error".into()];

        s.accept_scan(parse(Manager::Vscode, false, r#"{"profiles":[]}"#).unwrap());

        assert_eq!(s.inventory.discovery_errors, ["winget: other error"]);
        let rows: Vec<_> = s.inventory.scans.iter().flat_map(|scan| &scan.packages).collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].manager, Manager::VscodeInsiders);
    }
}
