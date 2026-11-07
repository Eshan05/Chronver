use crate::model::State;
use chrono::{NaiveDate, Utc};
use releasescout_engine::{Note, Report};
use rune::Latest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: usize = 2 * 1024 * 1024;

pub fn hash(value: impl Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).expect("serializable identity"))
    )
}

fn note_key(note: &Note) -> String {
    // Edits to a release body must not turn the same release into a new notification.
    hash((&note.url, note.version.as_ref().unwrap_or(&note.title)))
}

fn authored(note: &Note) -> bool {
    note.authored_notes && !note.body.trim().is_empty()
}

pub fn record_check(checks: &mut BTreeMap<String, String>, key: String, time: &str) -> bool {
    let Ok(date) = chrono::DateTime::parse_from_rfc3339(time) else {
        return false;
    };
    if checks
        .get(&key)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .is_some_and(|old| old >= date)
    {
        return false;
    }
    checks.insert(key, time.into());
    true
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    seen: BTreeSet<String>,
    newest: Option<NaiveDate>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Data {
    receipts: BTreeMap<String, Receipt>,
    pub candidate_checks: BTreeMap<String, String>,
    pub source_checks: BTreeMap<String, String>,
    #[serde(default)]
    pub icon_choices: BTreeMap<String, crate::icon_picker::Choice>,
    #[serde(default)]
    pub description_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub name_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub sidebar_filters: [crate::sidebar_filters::Facets; 3],
    #[serde(default)]
    sidebar_filter_defaults_set: bool,
}

impl Data {
    pub fn initialize_sidebar_filters(&mut self) -> bool {
        if self.sidebar_filter_defaults_set {
            return false;
        }
        let software = &mut self.sidebar_filters[crate::model::LibrarySection::Software as usize];
        if software.update == crate::sidebar_filters::Update::Any {
            software.update = crate::sidebar_filters::Update::Available;
        }
        self.sidebar_filter_defaults_set = true;
        true
    }

    pub fn unread(&self, feed: &str, report: &Report) -> BTreeSet<String> {
        let receipt = self.receipts.get(feed);
        report
            .notes
            .iter()
            .filter(|n| authored(n))
            .filter(|n| n.date.is_none_or(|date| date <= Utc::now().date_naive()))
            .filter(|n| {
                !receipt.is_some_and(|r| {
                    r.seen.contains(&note_key(n))
                        || n.date
                            .zip(r.newest)
                            .is_some_and(|(date, newest)| date < newest)
                })
            })
            .map(note_key)
            .collect()
    }

    pub fn acknowledge(&mut self, feed: &str, notes: &[Note], reading: bool) -> bool {
        let receipt = self.receipts.entry(feed.into()).or_default();
        let mut changed = false;
        for n in notes.iter().filter(|n| authored(n)) {
            changed |= receipt.seen.insert(note_key(n));
            if reading
                && n.date.is_some_and(|date| {
                    Some(date) > receipt.newest && date <= Utc::now().date_naive()
                })
            {
                receipt.newest = n.date;
                changed = true;
            }
        }
        changed
    }
}

#[derive(Default)]
pub struct Activity {
    pub data: Data,
    pub save: Latest<Result<(), String>>,
    pub error: Option<String>,
    persist: bool,
    blocked: bool,
    pub dirty: bool,
}

impl Activity {
    pub fn writable(&self) -> bool {
        !self.blocked
    }
}

fn path() -> PathBuf {
    crate::live::cache_directory().with_file_name("activity.json")
}

fn validate(data: &Data) -> Result<(), String> {
    let valid_key = |key: &str| key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit());
    if data.sidebar_filters.iter().any(|f| !f.valid())
        || data.receipts.len() > 4096
        || data.candidate_checks.len() > 10000
        || data.source_checks.len() > 4096
        || data.icon_choices.len() > 10000
        || data.description_overrides.len() > 10000
        || data.name_overrides.len() > 10000
        || data.name_overrides.iter().any(|(key, name)| {
            !valid_key(key) || crate::ai_metadata::name(name).as_deref() != Some(name.as_str())
        })
        || data
            .description_overrides
            .iter()
            .any(|(key, text)| !valid_key(key) || !crate::description_ui::valid(text))
        || data
            .icon_choices
            .iter()
            .any(|(k, v)| !valid_key(k) || !v.valid())
        || data.receipts.iter().any(|(k, r)| {
            !valid_key(k) || r.seen.len() > 4096 || r.seen.iter().any(|k| !valid_key(k))
        })
        || data
            .candidate_checks
            .iter()
            .chain(&data.source_checks)
            .any(|(k, v)| !valid_key(k) || chrono::DateTime::parse_from_rfc3339(v).is_err())
    {
        return Err("Invalid or oversized activity records".into());
    }
    Ok(())
}

fn load(path: &Path) -> Result<Data, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Data::default()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("Activity file exceeds 2 MiB".into());
    }
    let data = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate(&data)?;
    Ok(data)
}

fn save(path: &Path, data: &Data) -> Result<(), String> {
    validate(data)?;
    let bytes = serde_json::to_vec(data).map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("Activity file exceeds 2 MiB".into());
    }
    let parent = path.parent().ok_or("Activity parent missing")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

impl State {
    pub fn restore_activity(&mut self) {
        self.activity.persist = true;
        match load(&path()) {
            Ok(data) => self.activity.data = data,
            Err(e) => {
                self.activity.blocked = true;
                self.activity.error = Some(format!(
                    "Preferences unavailable; saved file left unchanged: {e}"
                ));
            }
        }
        self.activity.dirty |= self.activity.data.initialize_sidebar_filters();
        self.show_components = self.activity.data.sidebar_filters[0].include_support;
        self.set_dataset(self.dataset);
    }

    pub fn capture_note_feed(&mut self, id: &str, append: bool) {
        self.capture_note_cache(id,append);
        let p = self.catalog.packages.iter().find(|p| p.id == id).unwrap();
        let q = &self.live.sessions[id];
        let profile = self.live.catalog.packages.iter().find(|p| p.id == id);
        let stream = profile.and_then(|p| p.streams.get(q.component));
        let key = if append { q.report_feed.clone() } else { None }.unwrap_or_else(|| {
            hash((
                p.group_key
                    .as_deref()
                    .or(p.reference_id.as_deref())
                    .unwrap_or(&p.id),
                stream.map(|s| (&s.id, &s.sources)),
                q.channel,
                q.extension_published,
            ))
        });
        self.live.sessions.get_mut(id).unwrap().pending_feed = Some(key);
    }

    pub fn mark_current_notes_read(&mut self) {
        if self.inventory.open || self.background_watch.in_workspace() {
            return;
        }
        let Some(p) = self.selected().filter(|p| !p.sample_notes) else {
            return;
        };
        let Some(q) = self.live.sessions.get(&p.id) else {
            return;
        };
        let (Some(feed), Some(report)) = (&q.report_feed, &q.report) else {
            return;
        };
        let (feed, report) = (feed.clone(), report.clone());
        self.activity.dirty |= self.activity.data.acknowledge(&feed, &report.notes, true);
    }

    pub fn poll_activity(&mut self) -> bool {
        let mut changed = false;
        if let Some(result) = self.activity.save.take() {
            self.activity.error = result.err().map(|e| format!("Preferences not saved: {e}"));
            changed = true;
        }
        if self.activity.persist
            && !self.activity.blocked
            && self.activity.dirty
            && !self.activity.save.is_pending()
        {
            self.activity.dirty = false;
            self.activity
                .save
                .request(self.activity.data.clone(), |data| save(&path(), &data));
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn software_defaults_to_updates_once_without_hiding_extensions_or_overwriting_saved_choices() {
        use crate::sidebar_filters::Update;
        let mut data = Data::default();
        data.sidebar_filters[0].source = Some(true);
        assert!(data.initialize_sidebar_filters());
        assert_eq!(data.sidebar_filters[0].update, Update::Available);
        assert_eq!(data.sidebar_filters[0].source, Some(true));
        assert_eq!(data.sidebar_filters[1].update, Update::Any);
        assert_eq!(data.sidebar_filters[2].update, Update::Any);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("default-filters.json");
        data.sidebar_filters[0].update = Update::Any;
        save(&path, &data).unwrap();
        let mut restored = load(&path).unwrap();
        assert!(!restored.initialize_sidebar_filters());
        assert_eq!(restored.sidebar_filters[0].update, Update::Any);
        let mut previous = Data::default();
        previous.sidebar_filters[0].update = Update::Unknown;
        previous.initialize_sidebar_filters();
        assert_eq!(previous.sidebar_filters[0].update, Update::Unknown);
    }
    fn note(version: &str, date: Option<&str>) -> Note {
        Note {
            title: version.into(),
            version: Some(version.into()),
            url: format!("https://example.com/{version}"),
            body: "Fixed it".into(),
            authored_notes: true,
            date: date.map(|s| s.parse().unwrap()),
            preview: false,
        }
    }
    fn report(notes: Vec<Note>) -> Report {
        Report {
            package: "fixture".into(),
            stream: "main".into(),
            scope: "Fixture".into(),
            basis: releasescout_engine::Basis::Version,
            status: "partial".into(),
            complete: false,
            notes,
            warnings: vec![],
            evidence: vec![],
            attempts: vec![],
        }
    }
    #[test]
    fn receipts_survive_reload_and_do_not_conflate_edits_streams_or_old_history() {
        let mut data = Data::default();
        let feed = hash("stable");
        let mut r = report(vec![note("2.0", Some("2025-01-10")), note("1.0", None)]);
        assert_eq!(data.unread(&feed, &r).len(), 2);
        assert!(data.acknowledge(&feed, &r.notes, true));
        r.notes[0].body = "Edited body".into();
        r.notes.push(note("0.9", Some("2025-01-09")));
        assert!(data.unread(&feed, &r).is_empty());
        r.notes.push(note("2.1", Some("2025-01-10")));
        assert_eq!(data.unread(&feed, &r).len(), 1);
        assert_eq!(data.unread(&hash("preview"), &r).len(), 4);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("activity.json");
        save(&file, &data).unwrap();
        save(&file, &data).unwrap();
        assert_eq!(load(&file).unwrap().unread(&feed, &r).len(), 1);
    }
    #[test]
    fn icon_preferences_roundtrip_and_old_activity_files_remain_compatible() {
        use crate::icon_picker::Choice;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("activity.json");
        std::fs::write(
            &file,
            br#"{"receipts":{},"candidate_checks":{},"source_checks":{}}"#,
        )
        .unwrap();
        let mut data = load(&file).unwrap();
        assert!(data.icon_choices.is_empty());
        data.icon_choices
            .insert(hash("a"), Choice::Symbol("folder".into()));
        data.icon_choices.insert(hash("b"), Choice::App(hash("c")));
        save(&file, &data).unwrap();
        assert_eq!(load(&file).unwrap().icon_choices, data.icon_choices);
        data.icon_choices
            .insert(hash("bad"), Choice::Symbol("not-a-symbol".into()));
        assert!(save(&file, &data).is_err());
        assert_eq!(load(&file).unwrap().icon_choices.len(), 2);
    }

    #[test]
    fn sidebar_filters_roundtrip_per_tab_and_reject_unknown_providers() {
        use crate::sidebar_filters::{Facets, Update};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("filters.json");
        let mut data = Data::default();
        data.sidebar_filters[0] = Facets {
            update: Update::Available,
            source: Some(true),
            include_support: true,
            providers: ["scoop".into(), "winget".into()].into(),
            ..Default::default()
        };
        data.sidebar_filters[1].notes = Some(false);
        data.sidebar_filters[2].source = Some(false);
        save(&path, &data).unwrap();
        assert_eq!(load(&path).unwrap().sidebar_filters, data.sidebar_filters);
        let before = std::fs::read(&path).unwrap();
        data.sidebar_filters[0]
            .providers
            .insert("unrecognized-provider".into());
        assert!(save(&path, &data).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn description_overrides_roundtrip_and_invalid_updates_leave_the_file_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("activity.json");
        let mut data = Data::default();
        let key = hash("description-product");
        data.description_overrides
            .insert(key.clone(), "A personal description".into());
        save(&file, &data).unwrap();
        assert_eq!(
            load(&file).unwrap().description_overrides,
            data.description_overrides
        );
        data.description_overrides
            .insert(key.clone(), "x".repeat(crate::description_ui::LIMIT + 1));
        assert!(save(&file, &data).is_err());
        assert_eq!(
            load(&file).unwrap().description_overrides[&key],
            "A personal description"
        );
        let old: Data =
            serde_json::from_str(r#"{"receipts":{},"candidate_checks":{},"source_checks":{}}"#)
                .unwrap();
        assert!(old.description_overrides.is_empty());
        let mut s = State::new();
        s.activity.blocked = true;
        s.open_description_editor();
        assert!(s.description_editor.is_none());
    }

    #[test]
    fn metadata_only_notes_and_duplicate_records_do_not_inflate_unread() {
        let mut r = report(vec![note("1", None), note("1", None)]);
        assert_eq!(Data::default().unread("x", &r).len(), 1);
        for n in &mut r.notes {
            n.authored_notes = false;
        }
        assert!(Data::default().unread("x", &r).is_empty());
    }
    #[test]
    fn malformed_oversized_and_invalid_files_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("activity.json");
        std::fs::write(&file, b"broken").unwrap();
        assert!(load(&file).is_err());
        let mut data = Data::default();
        data.receipts.insert("bad".into(), Receipt::default());
        assert!(save(&file, &data).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"broken");
        std::fs::write(&file, vec![b' '; LIMIT + 1]).unwrap();
        assert!(load(&file).is_err());
    }

    #[test]
    fn a_feed_is_not_marked_read_while_inventory_is_open_and_requests_keep_their_source_scope() {
        let mut s = State::demo();
        s.select_package("pnpm");
        s.capture_note_feed("gh", false);
        let original = s.live.sessions["gh"].pending_feed.clone();
        s.live.sessions.get_mut("gh").unwrap().channel = 1;
        s.apply_engine_result("gh".into(), Ok(report(vec![note("1.0", None)])));
        assert_eq!(s.live.sessions["gh"].report_feed, original);
        s.select_package("gh");
        s.inventory.open = true;
        s.mark_current_notes_read();
        let q = &s.live.sessions["gh"];
        assert_eq!(
            s.activity
                .data
                .unread(q.report_feed.as_ref().unwrap(), q.report.as_ref().unwrap())
                .len(),
            1
        );
        s.inventory.open = false;
        s.mark_current_notes_read();
        let q = &s.live.sessions["gh"];
        assert!(
            s.activity
                .data
                .unread(q.report_feed.as_ref().unwrap(), q.report.as_ref().unwrap())
                .is_empty()
        );
        s.capture_note_feed("gh", false);
        assert_ne!(s.live.sessions["gh"].pending_feed, original);
    }

    #[test]
    fn successful_checks_are_monotonic_and_future_publications_are_not_notifications() {
        let mut checks = BTreeMap::new();
        let key = hash("fixture");
        assert!(record_check(
            &mut checks,
            key.clone(),
            "2025-01-11T00:00:00Z"
        ));
        assert!(!record_check(
            &mut checks,
            key.clone(),
            "2025-01-10T00:00:00Z"
        ));
        assert!(!record_check(&mut checks, key, "invalid"));
        let future = report(vec![note("future", Some("2999-01-01"))]);
        assert!(Data::default().unread("x", &future).is_empty());
    }
}
