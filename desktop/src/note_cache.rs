use crate::{live::Query, model::State};
use chrono::{DateTime, Utc};
use releasescout_engine::{
    HistoryCursor, HistoryPage, Report, Source, Stream,
    http::{Document, Fetcher},
};
use rune::{Latest, TextEdit};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

// Bump when extraction semantics change independently of the source profile.
const REVISION: u32 = 1;
const LIMIT: usize = 8 * 1024 * 1024;
const TTL: i64 = 15 * 60;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    component: usize,
    channel: usize,
    range: bool,
    from: String,
    to: String,
    extension_published: bool,
    inventory_range: bool,
}
impl Selection {
    fn capture(q: &Query) -> Self {
        Self {
            component: q.component,
            channel: q.channel,
            range: q.range,
            from: q.from.text.clone(),
            to: q.to.text.clone(),
            extension_published: q.extension_published,
            inventory_range: q.inventory_range,
        }
    }
    fn apply(&self, q: &mut Query) {
        q.component = self.component;
        q.channel = self.channel;
        q.range = self.range;
        q.from = TextEdit::new(&self.from);
        q.to = TextEdit::new(&self.to);
        q.extension_published = self.extension_published;
        q.inventory_range = self.inventory_range;
    }

    fn matches_request(&self, other: &Self) -> bool {
        self.range == other.range
            && self.component == other.component
            && self.channel == other.channel
            && self.extension_published == other.extension_published
            && (!self.range
                || (self.from.trim() == other.from.trim() && self.to.trim() == other.to.trim()))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    id: String,
    key: String,
    selection: Selection,
    local_file: Option<PathBuf>,
    components: usize,
}

impl Context {
    pub(crate) fn for_version_range(mut self, from: &str, to: &str) -> Self {
        self.selection.range = true;
        self.selection.from = from.into();
        self.selection.to = to.into();
        self
    }

    pub(crate) fn identity(&self) -> &str {
        &self.key
    }

    pub(crate) fn range_bounds(&self) -> Option<(&str, &str)> {
        let from = self.selection.from.trim();
        let to = self.selection.to.trim();
        (self.selection.range && !from.is_empty() && !to.is_empty()).then_some((from, to))
    }

    pub fn matches_version_range(&self, from: &str, to: &str, query: &Query) -> bool {
        self.selection.range
            && self.selection.from.trim() == from.trim()
            && self.selection.to.trim() == to.trim()
            && self.selection.channel == query.channel
            && self.selection.component == query.component
    }
}

#[cfg(test)]
fn has_fresh_range(directory: &Path, expected: &Context) -> bool {
    expected.selection.range
        && matching_entry(directory, expected).is_some_and(|entry| entry.fresh(Utc::now()))
}

pub fn context(s: &State, id: &str) -> Option<Context> {
    let p = s.catalog.packages.iter().find(|p| p.id == id)?;
    let q = s.live.sessions.get(id)?;
    let profile = s.live.catalog.packages.iter().find(|p| p.id == id);
    let key = crate::activity::hash((
        REVISION,
        &p.id,
        &p.installed,
        &p.target,
        profile,
        &p.source_hints,
        &p.extension,
        &p.dependency,
        &p.watchlist,
        crate::activity::hash(std::env::var("GITHUB_TOKEN").unwrap_or_default()),
    ));
    // Zod's old saved report came from a migration guide, not its release source.
    let uses_zod = p.watchlist.as_ref().is_some_and(|e| e.name == "zod")
        || p.dependency.as_ref().is_some_and(|d| d.package == "zod");
    let key = if uses_zod {
        crate::activity::hash((key, "zod-release-source"))
    } else {
        key
    };
    Some(Context {
        id: id.into(),
        key,
        selection: Selection::capture(q),
        local_file: p.extension.as_ref().and_then(|e| e.changelog.clone()),
        components: profile.map_or(1, |p| p.streams.len().max(1)),
    })
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Stamp {
    bytes: u64,
    modified: u128,
}
fn stamp(path: &Path) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    Some(Stamp {
        bytes: m.len(),
        modified: m
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos(),
    })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    revision: u32,
    context: Context,
    saved_at: DateTime<Utc>,
    report: Report,
    next: Option<HistoryCursor>,
    feed: Option<String>,
    caption: String,
    local_stamp: Option<Stamp>,
}
impl Entry {
    fn fresh(&self, now: DateTime<Utc>) -> bool {
        (0..TTL).contains(&(now.timestamp() - self.saved_at.timestamp()))
            && self.report.evidence.iter().all(|e| {
                !e.cache.starts_with("stale_cache")
                    && e.cache != "offline_cache"
                    && DateTime::parse_from_rfc3339(&e.fetched_at)
                        .is_ok_and(|time| (0..TTL).contains(&(now.timestamp() - time.timestamp())))
            })
    }
}

fn cache_path(directory: &Path, context: &Context) -> PathBuf {
    let suffix = if context.selection.range { "-range" } else { "-history" };
    slot_path(directory, context, suffix)
}

fn slot_path(directory: &Path, context: &Context, suffix: &str) -> PathBuf {
    directory.join(format!("{}{suffix}.json", crate::activity::hash(&context.id)))
}

fn load_entry(path: &Path, expected: &Context) -> Result<Option<Entry>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("Saved notes exceed 8 MiB".into());
    }
    let entry: Entry = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if entry.revision != REVISION
        || entry.context.key != expected.key
        || entry.context.id != expected.id
        || entry.report.package != expected.id
        || entry.context.selection.channel > 2
        || entry.context.selection.component >= expected.components
        || entry.context.local_file != expected.local_file
        || entry.context.selection.from.len() > 1024
        || entry.context.selection.to.len() > 1024
        || entry.report.status == "source_failed"
        || entry.saved_at > Utc::now()
    {
        return Ok(None);
    }
    if !entry.context.selection.extension_published
        && let Some(path) = &entry.context.local_file
        && (entry.local_stamp.is_none() || entry.local_stamp != stamp(path))
    {
        return Ok(None);
    }
    Ok(Some(entry))
}

fn load(directory: &Path, expected: &Context) -> Result<Option<Entry>, String> {
    // The legacy last-result file remains readable; opening a package still shows
    // its most recent compatible result, regardless of the draft query controls.
    let mut paths = ["-history", "-range", ""].map(|suffix| {
        let path = slot_path(directory, expected, suffix);
        let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
        (path, modified)
    });
    paths.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    let mut error = None;
    for (path, _) in paths {
        match load_entry(&path, expected) {
            Ok(Some(entry)) => return Ok(Some(entry)),
            Ok(None) => {}
            Err(e) => error = Some(e),
        }
    }
    error.map_or(Ok(None), Err)
}

fn matching_entry(directory: &Path, expected: &Context) -> Option<Entry> {
    [
        cache_path(directory, expected),
        slot_path(directory, expected, ""),
    ]
    .iter()
    .filter_map(|path| load_entry(path, expected).ok().flatten())
    .find(|entry| {
        entry.context.selection.matches_request(&expected.selection)
            && entry.report.notes.iter().any(|note| note.authored_notes)
            && !entry.report.evidence.is_empty()
    })
}

fn page(entry: Entry) -> HistoryPage {
    HistoryPage {
        report: entry.report,
        next: entry.next,
    }
}

pub(crate) fn single_document(stream: &Stream) -> Option<String> {
    match stream.sources.as_slice() {
        [Source::Document { url, .. }] if stream.npm_date_package.is_none() => Some(url.clone()),
        _ => None,
    }
}

type FetchResult = releasescout_engine::Result<Document>;

pub(crate) struct FirstDocument<F> {
    inner: F,
    resource: RefCell<Option<(String, FetchResult)>>,
}

impl<F: Fetcher> Fetcher for FirstDocument<F> {
    fn npm_dates(&self, package: &str) -> FetchResult {
        self.inner.npm_dates(package)
    }

    fn get(&self, url: &str) -> FetchResult {
        let mut resource = self.resource.borrow_mut();
        if resource.as_ref().is_some_and(|(saved, _)| saved == url) {
            return resource.take().unwrap().1;
        }
        self.inner.get(url)
    }
}

// The second result signals a fresh parsed-cache hit with no source request.
pub(crate) fn fetch_page<F: Fetcher>(
    saved: Option<(PathBuf, Context)>,
    url: Option<String>,
    connect: impl FnOnce() -> releasescout_engine::Result<F>,
    fetch: impl FnOnce(FirstDocument<F>) -> releasescout_engine::Result<HistoryPage>,
) -> releasescout_engine::Result<(HistoryPage, bool)> {
    let mut saved = saved.and_then(|(directory, context)| matching_entry(&directory, &context));
    if saved.as_ref().is_some_and(|entry| entry.fresh(Utc::now())) {
        let mut entry = saved.take().unwrap();
        mark_cached(&mut entry.report, true);
        return Ok((page(entry), true));
    }
    let inner = connect()?;
    let mut resource = None;
    if let (Some(mut entry), Some(url)) = (saved, url)
        && let [original] = entry.report.evidence.as_slice()
        && let Some(fingerprint) = original.content_sha256.as_deref()
        && !may_need_github_dates(&url, &entry.report)
    {
        let result = inner.get(&url);
        if let Ok(doc) = &result
            && matches!(
                doc.evidence.cache.as_str(),
                "network" | "revalidated_cache" | "fresh_cache"
            )
            && doc.evidence.url == original.url
            && doc.content_sha256() == fingerprint
        {
            let mut evidence = doc.evidence.clone();
            evidence.content_sha256 = Some(fingerprint.to_owned());
            entry.report.evidence[0] = evidence;
            return Ok((page(entry), false));
        } else {
            resource = Some((url, result));
        }
    }
    let page = fetch(FirstDocument {
        inner,
        resource: RefCell::new(resource),
    })?;
    Ok((page, false))
}

fn may_need_github_dates(url: &str, report: &Report) -> bool {
    releasescout_engine::http::https(url).is_ok_and(|url| {
        matches!(
            url.host_str(),
            Some("github.com" | "raw.githubusercontent.com")
        ) && report
            .notes
            .iter()
            .any(|n| n.authored_notes && n.date.is_none() && n.version.is_some())
    })
}

pub(crate) fn is_saved(report: &Report) -> bool {
    !report.evidence.is_empty()
        && report.evidence.iter().all(|e| {
            e.cache.starts_with("parsed_cache") || e.cache.starts_with("stale_cache_parsed")
        })
}

fn mark_cached(report: &mut Report, fresh: bool) {
    for e in &mut report.evidence {
        e.cache = format!(
            "{} ({})",
            if fresh { "parsed_cache" } else { "stale_cache_parsed" },
            e.cache
        );
    }
    if !fresh {
        report.complete = false;
        if report.status == "available" {
            report.status = "partial".into();
        }
        report.warnings.push("Saved notes; upstream freshness has not been rechecked".into());
    }
}

fn save(directory: &Path, mut entry: Entry) -> Result<(), String> {
    entry.local_stamp = entry
        .context
        .local_file
        .as_deref()
        .filter(|_| !entry.context.selection.extension_published)
        .and_then(stamp);
    let bytes = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("Parsed notes exceed 8 MiB; result not persisted".into());
    }
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(cache_path(directory, &entry.context))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub struct Cache {
    pub enabled: bool,
    pub read: Latest<(Context, usize, Result<Option<Entry>, String>)>,
    pub write: Latest<Result<(), String>>,
    pub error: Option<String>,
    directory: PathBuf,
    attempted: HashSet<String>,
    pending: VecDeque<Entry>,
    restoring: bool,
    // Latest settles before take(); keep completed answers until they are consumed.
    read_active: bool,
    write_active: bool,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            enabled: false,
            read: Latest::default(),
            write: Latest::default(),
            error: None,
            directory: crate::live::cache_directory().with_file_name("notes"),
            attempted: HashSet::new(),
            pending: VecDeque::new(),
            restoring: false,
            read_active: false,
            write_active: false,
        }
    }
}

impl Cache {
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
}

impl State {
    pub(crate) fn fresh_note_lookup(&self, id: &str) -> Option<(PathBuf, Context)> {
        let q = &self.live.sessions[id];
        if !self.note_cache.enabled || q.cache != 0 {
            return None;
        }
        q.pending_cache
            .clone()
            .map(|context| (self.note_cache.directory.clone(), context))
    }

    pub fn capture_note_cache(&mut self, id: &str, append: bool) {
        let captured = if append {
            self.live.sessions[id].report_cache.clone()
        } else {
            None
        }
        .or_else(|| context(self, id));
        self.live.sessions.get_mut(id).unwrap().pending_cache = captured;
    }
    pub fn persist_notes(&mut self, id: &str) {
        if !self.note_cache.enabled || self.note_cache.restoring {
            return;
        }
        let q = &self.live.sessions[id];
        let (Some(context), Some(report)) = (&q.report_cache, &q.report) else {
            return;
        };
        if report.status == "source_failed" || is_saved(report) {
            return;
        }
        let entry = Entry {
            revision: REVISION,
            context: context.clone(),
            saved_at: Utc::now(),
            report: report.clone(),
            next: q.next.clone(),
            feed: q.report_feed.clone(),
            caption: q.caption.trim_end_matches(" / saved notes").to_owned(),
            local_stamp: None,
        };
        self.note_cache
            .pending
            .retain(|e| e.context.id != context.id || e.context.selection.range != context.selection.range);
        self.note_cache.pending.push_back(entry);
    }

    pub fn poll_note_cache(&mut self) -> bool {
        if !self.note_cache.enabled {
            return false;
        }
        let mut changed = false;
        if let Some(result) = self.note_cache.write.take() {
            self.note_cache.write_active = false;
            self.note_cache.error = result.err().map(|e| format!("Notes not saved: {e}"));
            changed = true;
        }
        if let Some((guard, cache_mode, result)) = self.note_cache.read.take() {
            self.note_cache.read_active = false;
            if let Err(e) = &result {
                self.note_cache.error = Some(format!("Saved notes unavailable: {e}"));
            }
            let current = self.selected().and_then(|p| context(self, &p.id));
            if current.as_ref().is_none_or(|c| c.id != guard.id) {
                // A discarded read must not prevent restoration when returning to the package.
                self.note_cache.attempted.remove(&guard.key);
            }
            let matches = current.is_some_and(|c| {
                c.key == guard.key && c.id == guard.id && c.selection == guard.selection
            }) && self.live.active.is_none()
                && self.live.sessions.get(&guard.id).is_some_and(|q| {
                    q.report.is_none() && q.error.is_none() && q.cache == cache_mode
                });
            if matches && let Ok(Some(mut entry)) = result {
                let fresh = entry.fresh(Utc::now());
                let q = self.live.sessions.get_mut(&guard.id).unwrap();
                entry.context.selection.apply(q);
                q.next = entry.next;
                q.caption = if entry.caption.is_empty() {
                    "Saved notes".into()
                } else {
                    format!("{} / saved notes", entry.caption)
                };
                q.pending_feed = entry.feed;
                q.pending_cache = Some(entry.context);
                mark_cached(&mut entry.report, fresh);
                self.note_cache.restoring = true;
                self.apply_engine_result(guard.id.clone(), Ok(entry.report));
                self.live.sessions.get_mut(&guard.id).unwrap().restored = true;
                self.note_cache.restoring = false;
            }
            changed = true;
        }
        if !self.note_cache.write_active
            && let Some(entry) = self.note_cache.pending.pop_front()
        {
            let directory = self.note_cache.directory.clone();
            self.note_cache.write_active = true;
            self.note_cache
                .write
                .request(entry, move |entry| save(&directory, entry));
        }
        if !self.note_cache.read_active && !self.inventory.open && self.live.active.is_none() {
            let candidate = self.selected().filter(|p| !p.sample_notes).and_then(|p| {
                let q = self.live.sessions.get(&p.id)?;
                (q.report.is_none() && q.error.is_none() && q.cache != 1)
                    .then(|| (context(self, &p.id), q.cache))
            });
            if let Some((Some(context), mode)) = candidate
                && self.note_cache.attempted.insert(context.key.clone())
            {
                let directory = self.note_cache.directory.clone();
                self.note_cache.read_active = true;
                self.note_cache
                    .read
                    .request((context, mode), move |(context, mode)| {
                        let result = load(&directory, &context);
                        (context, mode, result)
                    });
            }
        }
        changed
    }
}

pub fn saved_scene(s: &mut State) {
    s.inventory.scans = releasescout_engine::matching::load_snapshot(
        &crate::live::cache_directory().with_file_name("inventory.json"),
    )
    .expect("saved inventory");
    s.rebuild_detected();
    let id = s
        .catalog
        .packages
        .iter()
        .find(|p| {
            p.reference_id.as_deref() == Some("gcloud")
                && p.provider == Some(releasescout_engine::inventory::Manager::Scoop)
        })
        .expect("saved gcloud observation")
        .id
        .clone();
    s.select_package(&id);
    s.note_cache.enabled = true;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        s.poll_note_cache();
        if !s.note_cache.read_active {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "saved notes read did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        s.live.sessions[&id].restored,
        "run saved_gcloud_parsed_cache_roundtrip first"
    );
    s.note_cache.enabled = false;
}

pub fn saved_range_scene(s: &mut State) {
    crate::update_status::scene(s);
    let id = s.selected().unwrap().id.clone();
    let selection = context(s, &id).unwrap().for_version_range("2.0.0", "2.2.0");
    let q = s.engine_query();
    selection.selection.apply(q);
    q.report_cache = Some(selection);
    q.restored = true;
    q.caption = "(2.0.0, 2.2.0] / version-notes / saved notes".into();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "explicit gcloud source/cache integration; reads saved inventory and refreshes HTTP/parsed caches only"]
    fn saved_gcloud_parsed_cache_roundtrip() {
        let scans = releasescout_engine::matching::load_snapshot(
            &crate::live::cache_directory().with_file_name("inventory.json"),
        )
        .unwrap();
        fn detected(scans: &[releasescout_engine::inventory::Scan]) -> State {
            let mut s = State::new();
            s.inventory.scans = scans.to_vec();
            s.rebuild_detected();
            let id = s
                .catalog
                .packages
                .iter()
                .find(|p| {
                    p.reference_id.as_deref() == Some("gcloud")
                        && p.provider == Some(releasescout_engine::inventory::Manager::Scoop)
                })
                .unwrap()
                .id
                .clone();
            s.select_package(&id);
            s.note_cache.enabled = true;
            s
        }
        let mut first = detected(&scans);
        let id = first.selected().unwrap().id.clone();
        first.engine_query().range = false;
        first.engine_query().cache = 1;
        first.fetch_notes();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if first.poll_engine() | first.poll_history() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "gcloud request did not settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            first.live.sessions[&id].error.is_none(),
            "{:?}",
            first.live.sessions[&id].error
        );
        let report = first.live.sessions[&id].report.as_ref().unwrap();
        assert!(!report.notes.is_empty());
        assert!(
            report
                .evidence
                .iter()
                .all(|e| !e.cache.starts_with("stale_cache"))
        );
        let expected = serde_json::to_value(&report.notes).unwrap();
        settle(&mut first);
        let mut second = detected(&scans);
        let started = std::time::Instant::now();
        settle(&mut second);
        assert!(second.live.sessions[&id].restored);
        assert_eq!(
            serde_json::to_value(&second.live.sessions[&id].report.as_ref().unwrap().notes)
                .unwrap(),
            expected
        );
        assert!(!second.live.work.is_pending() && !second.live.history.is_pending());
        assert!(second.inventory.discovery.iter().all(|w| !w.is_pending()));
        assert!(!second.inventory.persist);
        eprintln!(
            "Restored {} gcloud releases from parsed cache in {:?}; no source or inventory worker started",
            second.selected().unwrap().releases.len(),
            started.elapsed()
        );
    }
    fn state(directory: &Path) -> State {
        let mut s = State::new();
        crate::update_status::scene(&mut s);
        s.note_cache.enabled = true;
        s.note_cache.directory = directory.into();
        s
    }
    fn blank(s: &mut State) {
        let id = s.selected().unwrap().id.clone();
        let q = s.live.sessions.get_mut(&id).unwrap();
        q.report = None;
        q.report_cache = None;
        q.pending_cache = None;
        q.pending_feed = None;
        q.report_feed = None;
        q.next = None;
        s.catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .releases
            .clear();
    }
    fn settle(s: &mut State) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            s.poll_note_cache();
            if !s.note_cache.read_active
                && !s.note_cache.write_active
                && s.note_cache.pending.is_empty()
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "cache IO did not settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn alternating_history_and_range_reuse_distinct_fresh_results() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = state(dir.path());
        let id = s.selected().unwrap().id.clone();
        s.live.catalog.packages.iter_mut().find(|p| p.id == id).unwrap()
            .streams[0].sources.clear();
        let mut report = s.live.sessions[&id].report.clone().unwrap();
        report.evidence = vec![releasescout_engine::Evidence {
            url: "https://example.com/releases".into(),
            fetched_at: Utc::now().to_rfc3339(),
            cache: "network".into(),
            content_sha256: None,
        }];
        for (range, title) in [(false, "Saved history result"), (true, "Saved range result")] {
            s.engine_query().range = range;
            s.engine_query().from = TextEdit::new("1.0.0");
            s.engine_query().to = TextEdit::new("2.2.0");
            report.notes[0].title = title.into();
            let context = context(&s, &id).unwrap();
            save(dir.path(), Entry {
                revision: REVISION,
                context,
                saved_at: Utc::now(),
                report: report.clone(),
                next: None,
                feed: None,
                caption: String::new(),
                local_stamp: None,
            }).unwrap();
        }

        for (range, title) in [(false, "Saved history result"), (true, "Saved range result")] {
            s.engine_query().range = range;
            s.fetch_notes();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !(s.poll_engine() | s.poll_history()) {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let q = &s.live.sessions[&id];
            assert!(q.restored);
            assert_eq!(q.report.as_ref().unwrap().notes[0].title, title);
            assert!(s.note_cache.pending.is_empty(), "a cache hit must not renew freshness");
        }
        s.engine_query().cache = 1;
        s.capture_note_feed(&id, false);
        assert!(s.fresh_note_lookup(&id).is_none(), "Refresh must bypass parsed notes");
        let mut mismatched = context(&s, &id).unwrap();
        mismatched.selection.to = "2.3.0".into();
        assert!(matching_entry(dir.path(), &mismatched).is_none());
    }

    #[test]
    fn revalidated_document_reuses_notes_only_when_the_parsed_body_matches() {
        struct OneDocument<'a> {
            calls: &'a std::cell::Cell<usize>,
            body: &'a str,
        }
        const URL: &str = "https://example.com/changelog";
        impl Fetcher for OneDocument<'_> {
            fn get(&self, url: &str) -> FetchResult {
                assert_eq!(url, URL);
                self.calls.set(self.calls.get() + 1);
                Ok(Document {
                    body: self.body.into(),
                    headers: Default::default(),
                    evidence: releasescout_engine::Evidence {
                        url: URL.into(),
                        fetched_at: Utc::now().to_rfc3339(),
                        cache: "revalidated_cache".into(),
                        content_sha256: None,
                    },
                })
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let s = state(dir.path());
        let id = s.selected().unwrap().id.clone();
        let context = context(&s, &id).unwrap();
        let calls = std::cell::Cell::new(0);
        let original = OneDocument {
            calls: &calls,
            body: "original",
        }
        .get(URL)
        .unwrap();
        let mut report = s.live.sessions[&id].report.clone().unwrap();
        report.evidence = vec![original.evidence.clone()];
        report.evidence[0].content_sha256 = Some(original.content_sha256());
        save(
            dir.path(),
            Entry {
                revision: REVISION,
                context: context.clone(),
                saved_at: Utc::now() - chrono::Duration::seconds(TTL + 1),
                report: report.clone(),
                next: None,
                feed: None,
                caption: String::new(),
                local_stamp: None,
            },
        )
        .unwrap();
        for (body, should_parse) in [("original", false), ("different cached body", true)] {
            calls.set(0);
            let parsed = std::cell::Cell::new(false);
            let (result, from_saved) = fetch_page(
                Some((dir.path().to_owned(), context.clone())),
                Some(URL.into()),
                || {
                    Ok(OneDocument {
                        calls: &calls,
                        body,
                    })
                },
                |fetcher| {
                    parsed.set(true);
                    assert_eq!(fetcher.get(URL)?.body, body);
                    Ok(HistoryPage {
                        report: report.clone(),
                        next: None,
                    })
                },
            )
            .unwrap();
            assert_eq!(calls.get(), 1);
            assert_eq!(parsed.get(), should_parse);
            assert!(!from_saved);
            assert_eq!(result.report.notes[0].body, report.notes[0].body);
        }
    }

    #[test]
    fn successful_notes_restore_on_reopen_without_http_or_rewriting_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = state(dir.path());
        let id = first.selected().unwrap().id.clone();
        first.persist_notes(&id);
        settle(&mut first);
        let c = context(&first, &id).unwrap();
        let path = cache_path(dir.path(), &c);
        let before = fs::read(&path).unwrap();
        let mut second = state(dir.path());
        blank(&mut second);
        settle(&mut second);
        assert_eq!(second.selected().unwrap().releases[0].version, "2.2.0");
        assert!(
            second.live.sessions[&id]
                .caption
                .to_lowercase()
                .ends_with("saved notes")
        );
        assert_eq!(
            crate::update_status::publisher(&second, second.selected().unwrap()).value,
            "2.2.0 / cached"
        );
        assert!(!second.live.work.is_pending() && !second.live.history.is_pending());
        assert_eq!(before, fs::read(path).unwrap());
        assert!(second.note_cache.error.is_none());
    }
    #[test]
    fn prefetch_reuses_only_fresh_exact_range_cache() {
        let dir = tempfile::tempdir().unwrap();
        let s = state(dir.path());
        let id = s.selected().unwrap().id.clone();
        let mut report = s.live.sessions[&id].report.clone().unwrap();
        let guard = context(&s, &id).unwrap();
        let range = guard.clone().for_version_range("1.0.0", "2.2.0");
        let now = Utc::now();
        if report.evidence.is_empty() {
            report.evidence.push(releasescout_engine::Evidence {
                url: "https://example.com/releases".into(),
                fetched_at: now.to_rfc3339(),
                cache: "network".into(),
                content_sha256: None,
            });
        }
        for item in &mut report.evidence {
            item.fetched_at = now.to_rfc3339();
            item.cache = "network".into();
        }
        save(
            dir.path(),
            Entry {
                revision: REVISION,
                context: range.clone(),
                saved_at: now,
                report,
                next: None,
                feed: None,
                caption: String::new(),
                local_stamp: None,
            },
        )
        .unwrap();
        assert!(has_fresh_range(dir.path(), &range));
        assert!(!has_fresh_range(dir.path(), &guard.for_version_range("1.0.0", "2.3.0")));
        let mut entry = load(dir.path(), &range).unwrap().unwrap();
        entry.saved_at -= chrono::Duration::seconds(TTL + 1);
        save(dir.path(), entry).unwrap();
        assert!(!has_fresh_range(dir.path(), &range));
    }
    #[test]
    fn cache_keeps_request_selection_and_pagination_not_later_draft_controls() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = state(dir.path());
        let id = first.selected().unwrap().id.clone();
        let q = first.live.sessions.get_mut(&id).unwrap();
        let report = q.report.take().unwrap();
        q.range = true;
        q.channel = 1;
        q.from = TextEdit::new("1.0.0");
        q.to = TextEdit::new("2.2.0");
        first.capture_note_feed(&id, false);
        first.live.sessions.get_mut(&id).unwrap().channel = 2;
        first.apply_engine_result(id.clone(), Ok(report));
        settle(&mut first);
        let mut second = state(dir.path());
        blank(&mut second);
        settle(&mut second);
        let q = &second.live.sessions[&id];
        assert!(q.range);
        assert_eq!(q.channel, 1);
        assert_eq!(q.from.text, "1.0.0");
        assert_eq!(q.to.text, "2.2.0");
        assert!(q.report.is_some());
        let c = context(&second, &id).unwrap();
        let mut entry = load(dir.path(), &c).unwrap().unwrap();
        entry.next = Some(HistoryCursor {
            package: id.clone(),
            stream: "main".into(),
            source_index: 0,
            source_key: "fixture source".into(),
            page: 2,
            offset: 20,
        });
        save(dir.path(), entry).unwrap();
        let mut third = state(dir.path());
        blank(&mut third);
        settle(&mut third);
        assert_eq!(third.live.sessions[&id].next.as_ref().unwrap().offset, 20);
    }
    #[test]
    fn changed_install_or_profile_invalidates_notes_and_late_reads_do_not_replace_user_queries() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = state(dir.path());
        let id = first.selected().unwrap().id.clone();
        first.persist_notes(&id);
        settle(&mut first);
        let original = context(&first, &id).unwrap();
        let mut changed = state(dir.path());
        blank(&mut changed);
        changed
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .installed = Some("3.0.0".into());
        assert_ne!(context(&changed, &id).unwrap().key, original.key);
        settle(&mut changed);
        assert!(changed.live.sessions[&id].report.is_none());
        let mut changed = state(dir.path());
        blank(&mut changed);
        changed
            .live
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .streams[0]
            .scope = "Other component".into();
        assert_ne!(context(&changed, &id).unwrap().key, original.key);
        settle(&mut changed);
        assert!(changed.live.sessions[&id].report.is_none());
        let mut late = state(dir.path());
        blank(&mut late);
        late.poll_note_cache();
        late.live.sessions.get_mut(&id).unwrap().channel = 1;
        settle(&mut late);
        assert!(late.live.sessions[&id].report.is_none());
        assert_eq!(late.live.sessions[&id].channel, 1);
    }
    #[test]
    fn stale_offline_results_remain_labelled_stale_and_failures_do_not_replace_good_cache() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = state(dir.path());
        let id = first.selected().unwrap().id.clone();
        first.persist_notes(&id);
        settle(&mut first);
        let c = context(&first, &id).unwrap();
        let mut entry = load(dir.path(), &c).unwrap().unwrap();
        entry.saved_at = Utc::now() - chrono::Duration::hours(1);
        save(dir.path(), entry).unwrap();
        let path = cache_path(dir.path(), &c);
        let before = fs::read(&path).unwrap();
        let mut second = state(dir.path());
        blank(&mut second);
        second.live.sessions.get_mut(&id).unwrap().cache = 2;
        settle(&mut second);
        let report = second.live.sessions[&id].report.as_ref().unwrap();
        assert!(!report.complete);
        assert!(report.warnings.iter().any(|w| w.contains("Saved notes")));
        assert!(!second.live.work.is_pending());
        let mut failed = report.clone();
        failed.status = "source_failed".into();
        failed.notes.clear();
        second.apply_engine_result(id.clone(), Ok(failed));
        assert!(!second.selected().unwrap().releases.is_empty());
        second.apply_engine_result(id, Err("Fixture refresh failed".into()));
        settle(&mut second);
        assert_eq!(before, fs::read(path).unwrap());
        assert!(!second.selected().unwrap().releases.is_empty());
    }
    #[test]
    fn malformed_wrong_revision_oversized_and_invalid_component_entries_are_not_restored() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = state(dir.path());
        let id = s.selected().unwrap().id.clone();
        s.persist_notes(&id);
        settle(&mut s);
        let c = context(&s, &id).unwrap();
        let path = cache_path(dir.path(), &c);
        let valid = fs::read(&path).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        value["revision"] = serde_json::json!(REVISION + 1);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(load(dir.path(), &c).unwrap().is_none());
        value["revision"] = serde_json::json!(REVISION);
        value["context"]["selection"]["component"] = serde_json::json!(999);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(load(dir.path(), &c).unwrap().is_none());
        fs::write(&path, "{").unwrap();
        assert!(load(dir.path(), &c).is_err());
        fs::write(&path, vec![b' '; LIMIT + 1]).unwrap();
        assert!(load(dir.path(), &c).is_err());
    }
    #[test]
    fn explicit_refresh_skips_restoration_and_bad_files_surface_a_notice_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = state(dir.path());
        let id = first.selected().unwrap().id.clone();
        first.persist_notes(&id);
        settle(&mut first);
        let mut second = state(dir.path());
        blank(&mut second);
        second.live.sessions.get_mut(&id).unwrap().cache = 1;
        settle(&mut second);
        assert!(second.live.sessions[&id].report.is_none());
        assert!(!second.note_cache.read.is_pending());
        let c = context(&first, &id).unwrap();
        fs::write(cache_path(dir.path(), &c), "broken").unwrap();
        let mut third = state(dir.path());
        blank(&mut third);
        settle(&mut third);
        assert!(third.note_cache.error.is_some());
        assert!(!third.live.work.is_pending() && !third.live.history.is_pending());
    }
    fn extension_state(directory: &Path, root: &Path) -> State {
        let mut s = state(directory);
        let id = s.selected().unwrap().id.clone();
        s.catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .extension = Some(releasescout_engine::extensions::Installed {
            id: "acme.fixture".into(),
            root: Some(root.into()),
            changelog: Some(root.join("CHANGELOG.md")),
            ..Default::default()
        });
        let c = context(&s, &id).unwrap();
        s.live.sessions.get_mut(&id).unwrap().report_cache = Some(c);
        s
    }
    #[test]
    fn stale_auto_cache_restores_without_fetching_and_checks_local_file_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let changelog = root.path().join("CHANGELOG.md");
        fs::write(&changelog, "## 2.0.0\n\nFixed the startup crash.\n").unwrap();
        let mut first = extension_state(dir.path(), root.path());
        let id = first.selected().unwrap().id.clone();
        first.persist_notes(&id);
        settle(&mut first);
        let c = context(&first, &id).unwrap();
        let mut entry = load(dir.path(), &c).unwrap().unwrap();
        entry.saved_at = Utc::now() - chrono::Duration::days(2);
        save(dir.path(), entry).unwrap();

        let mut second = extension_state(dir.path(), root.path());
        blank(&mut second);
        assert_eq!(second.live.sessions[&id].cache, 0);
        settle(&mut second);
        let q = &second.live.sessions[&id];
        assert!(q.restored);
        assert!(
            q.report
                .as_ref()
                .unwrap()
                .warnings
                .iter()
                .any(|w| w.contains("Saved notes"))
        );
        assert_eq!(second.selected().unwrap().releases[0].version, "2.2.0");
        assert!(second.live.active.is_none());
        assert!(!second.live.work.is_pending() && !second.live.history.is_pending());

        fs::write(changelog, "## 2.0.0\n\nEdited notes.\n").unwrap();
        assert!(load(dir.path(), &c).unwrap().is_none());
    }

    #[test]
    fn returning_after_a_discarded_read_restores_saved_notes() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = state(dir.path());
        let id = first.selected().unwrap().id.clone();
        first.persist_notes(&id);
        settle(&mut first);

        let mut reopened = state(dir.path());
        blank(&mut reopened);
        reopened.poll_note_cache();
        assert!(reopened.note_cache.read_active);
        reopened.set_section(crate::model::LibrarySection::Extensions);
        settle(&mut reopened);
        assert!(reopened.live.sessions[&id].report.is_none());
        reopened.set_section(crate::model::LibrarySection::Software);
        reopened.select_package(&id);
        settle(&mut reopened);
        assert!(reopened.live.sessions[&id].restored);
    }
}
