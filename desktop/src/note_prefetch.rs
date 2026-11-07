use crate::{
    model::State,
    note_cache::{self, Context},
};
use releasescout_engine::{
    Catalog, Channel, Engine, Error, HistoryPage, Package, Report, Selection, Source,
    http::{CacheOptions, Document, Fetcher, HttpClient},
    version::Version,
};
use rune::{Latest, TextEdit, Waker};
use std::{
    cell::Cell,
    collections::{HashSet, VecDeque},
    path::Path,
    time::{Duration, SystemTime},
};

const CONCURRENCY: usize = 3;
const PER_HOST: usize = 2;
const RETRIES: u8 = 2;

#[derive(Clone)]
struct Job {
    profile: Package,
    context: Context,
    desired: Context,
    component: String,
    host: String,
    channel: Channel,
    from: String,
    to: String,
}

#[derive(Default)]
struct Slot {
    active: Option<Job>,
    work: Latest<Outcome>,
}

enum Outcome {
    Saved,
    Loaded(Report),
    Unavailable,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Progress {
    pub active: usize,
    pub loaded: usize,
    pub saved: usize,
    pub unavailable: usize,
}

#[derive(Default)]
pub struct Prefetch {
    pub enabled: bool,
    dirty: bool,
    queue: VecDeque<String>,
    attempted: HashSet<String>,
    slots: [Slot; CONCURRENCY],
    progress: Progress,
}

impl Prefetch {
    pub fn set_waker(&self, waker: Waker) {
        for slot in &self.slots {
            slot.work.set_waker(waker.clone());
        }
    }

    pub fn refresh(&mut self) {
        self.dirty = self.enabled;
    }

    pub fn loading(&self, id: &str) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.active.as_ref().is_some_and(|job| job.profile.id == id))
    }

    pub(crate) fn progress(&self) -> Progress {
        Progress {
            active: self
                .slots
                .iter()
                .filter(|slot| slot.active.is_some())
                .count(),
            ..self.progress
        }
    }
}

pub fn activity_scene(s: &mut State) {
    crate::inventory_view::scene(s);
    s.inventory.open = false;
    s.note_prefetch.slots[0].active = s.catalog.packages.iter().find_map(|p| job(s, &p.id));
    s.note_prefetch.progress.loaded = 8;
    s.note_prefetch.progress.saved = 3;
    s.note_prefetch.progress.unavailable = 2;
}

fn host(source: &Source) -> Option<String> {
    match source {
        Source::Github { .. } => Some("api.github.com".into()),
        Source::Document { url, .. }
        | Source::Feed { url }
        | Source::LinkedDocuments { url, .. } => {
            url::Url::parse(url).ok()?.host_str().map(str::to_owned)
        }
        Source::InstalledDocument { .. } => None,
    }
}

fn job(s: &State, id: &str) -> Option<Job> {
    let package = s.catalog.packages.iter().find(|p| p.id == id)?;
    if !eligible(package) {
        return None;
    }
    let q = s.live.sessions.get(id)?;
    if q.cache != 0 || q.error.is_some() || q.pending_cache.is_some() {
        return None;
    }
    let (from, to) = s.installed_update_range(id)?;
    if q.report.is_some() && !stale_range(q, from, to) {
        return None;
    }
    // Respect an explicitly selected history or a hand-edited range.
    if !q.inventory_range
        && (q.range || !q.from.text.is_empty() || !q.to.text.is_empty())
        && !(q.range && q.from.text.trim() == from && q.to.text.trim() == to)
    {
        return None;
    }
    if s.live.unavailable(id).is_some() {
        return None;
    }
    let profile = s.live.catalog.packages.iter().find(|p| p.id == id)?;
    let stream = profile.streams.get(q.component)?;
    let from_v = Version::parse(from, stream.scheme).ok()?;
    let to_v = Version::parse(to, stream.scheme).ok()?;
    if !to_v.compare(&from_v).is_gt() {
        return None;
    }
    let context = note_cache::context(s, id)?;
    Some(Job {
        profile: profile.clone(),
        desired: context.clone().for_version_range(from, to),
        context,
        component: stream.id.clone(),
        host: stream.sources.iter().find_map(host)?,
        channel: *[Channel::Stable, Channel::Preview, Channel::All].get(q.channel)?,
        from: from.into(),
        to: to.into(),
    })
}

fn eligible(package: &crate::model::Package) -> bool {
    package.detected
        && package.component_reason.is_none()
        && package
            .observation
            .as_ref()
            .is_some_and(|o| o.confirmed_update)
}

fn stale_range(q: &crate::live::Query, from: &str, to: &str) -> bool {
    q.restored
        && q.range
        && q.from.text.trim() == from
        && q.to.text.trim() == to
        && q.report_cache
            .as_ref()
            .is_some_and(|context| context.matches_version_range(from, to, q))
        && q.report.as_ref().is_some_and(|report| {
            report
                .evidence
                .iter()
                .any(|e| e.cache.starts_with("stale_cache_parsed"))
        })
}

fn retryable(error: &Error) -> bool {
    match error {
        Error::Network(e) => e.is_timeout() || e.is_connect(),
        Error::Http { status, .. } => matches!(status, 408 | 500..=599),
        // HttpClient honors Retry-After and GitHub's recorded rate-limit reset.
        _ => false,
    }
}

fn retry_delay(attempt: u8, jitter: u128) -> Duration {
    let base = 500_u64 << attempt.min(RETRIES);
    Duration::from_millis(base + (jitter % u128::from(base)) as u64)
}

struct WithRetry(HttpClient, Cell<u8>);

impl Fetcher for WithRetry {
    fn npm_dates(&self, package: &str) -> releasescout_engine::Result<Document> {
        self.0.npm_dates(package)
    }

    fn get(&self, url: &str) -> releasescout_engine::Result<Document> {
        loop {
            let result = self.0.get(url);
            let attempt = self.1.get();
            if attempt == RETRIES || !result.as_ref().is_err_and(|e| retryable(e)) {
                return result;
            }
            self.1.set(attempt + 1);
            let jitter = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            std::thread::sleep(retry_delay(attempt, jitter));
        }
    }
}

fn fetch(job: Job, directory: &Path) -> Outcome {
    let id = job.profile.id.clone();
    let document = job
        .profile
        .streams
        .iter()
        .find(|stream| stream.id == job.component)
        .and_then(note_cache::single_document);
    let catalog = Catalog {
        schema_version: 1,
        packages: vec![job.profile],
    };
    let options = CacheOptions {
        directory: crate::live::cache_directory(),
        public_only: true,
        github_token: std::env::var("GITHUB_TOKEN").ok(),
        ..Default::default()
    };
    let result = note_cache::fetch_page(
        Some((directory.to_owned(), job.desired)),
        document,
        || HttpClient::new(options).map(|client| WithRetry(client, Cell::new(0))),
        |fetcher| {
            Engine { catalog, fetcher }
                .notes(
                    &id,
                    Some(&job.component),
                    Selection::Versions {
                        from: job.from,
                        to: job.to,
                        channel: job.channel,
                    },
                    false,
                )
                .map(|report| HistoryPage { report, next: None })
        },
    );
    match result {
        Ok((_, true)) => Outcome::Saved,
        Ok((HistoryPage { report, .. }, false))
            if report.status != "source_failed" && report.notes.iter().any(|n| n.authored_notes) =>
        {
            Outcome::Loaded(report)
        }
        _ => Outcome::Unavailable,
    }
}

impl State {
    pub fn poll_note_prefetch(&mut self) -> bool {
        self.poll_note_prefetch_with(fetch)
    }

    fn poll_note_prefetch_with(&mut self, run: fn(Job, &Path) -> Outcome) -> bool {
        if !self.note_prefetch.enabled || self.live.active.is_some() {
            return false;
        }
        let mut changed = false;
        for index in 0..CONCURRENCY {
            let slot = &mut self.note_prefetch.slots[index];
            if slot.work.is_pending() {
                continue;
            }
            let Some(completed) = slot.active.take() else {
                continue;
            };
            changed = true;
            match slot.work.take() {
                Some(Outcome::Loaded(report)) => {
                    if self.apply_prefetched(completed, report) {
                        self.note_prefetch.progress.loaded += 1;
                    }
                }
                Some(Outcome::Saved) => self.note_prefetch.progress.saved += 1,
                Some(Outcome::Unavailable) => self.note_prefetch.progress.unavailable += 1,
                None => {}
            }
        }
        if self.note_prefetch.dirty {
            self.note_prefetch.dirty = false;
            let mut candidates: Vec<_> = self
                .catalog
                .packages
                .iter()
                .filter(|p| eligible(p))
                .collect();
            let selected = self.selected().map(|p| p.id.as_str());
            candidates.sort_by_key(|p| {
                (
                    Some(p.id.as_str()) != selected,
                    p.display_priority,
                    p.name.as_str(),
                )
            });
            self.note_prefetch.queue = candidates.into_iter().map(|p| p.id.clone()).collect();
        }
        if let Some(index) = self.selected().and_then(|package| {
            self.note_prefetch
                .queue
                .iter()
                .position(|id| id == &package.id)
        }) && let Some(package) = self.note_prefetch.queue.remove(index)
        {
            self.note_prefetch.queue.push_front(package);
        }
        let mut delayed = self.note_prefetch.queue.len();
        while delayed > 0 {
            let Some(index) = self
                .note_prefetch
                .slots
                .iter()
                .position(|slot| slot.active.is_none())
            else {
                break;
            };
            let Some(id) = self.note_prefetch.queue.pop_front() else {
                break;
            };
            delayed -= 1;
            let Some(task) = job(self, &id) else { continue };
            if self
                .note_prefetch
                .attempted
                .contains(task.desired.identity())
            {
                continue;
            }
            let active_hosts = self
                .note_prefetch
                .slots
                .iter()
                .filter_map(|slot| slot.active.as_ref());
            if active_hosts
                .filter(|active| active.host == task.host)
                .count()
                >= PER_HOST
            {
                self.note_prefetch.queue.push_back(id);
                continue;
            }
            self.note_prefetch
                .attempted
                .insert(task.desired.identity().into());
            let directory = self.note_cache.directory().to_owned();
            let slot = &mut self.note_prefetch.slots[index];
            slot.work
                .request(task.clone(), move |task| run(task, &directory));
            slot.active = Some(task);
            changed = true;
        }
        changed
    }

    fn apply_prefetched(&mut self, task: Job, report: Report) -> bool {
        let id = &task.profile.id;
        let Some(current) = note_cache::context(self, id) else {
            return false;
        };
        let q = &self.live.sessions[id];
        if q.cache != 0 || q.error.is_some() || q.pending_cache.is_some() {
            return false;
        }
        let fits = if q.report.is_none() {
            current == task.context
        } else {
            current.identity() == task.context.identity()
                && stale_range(q, &task.from, &task.to)
                && report.complete
        };
        if !fits {
            return false;
        }
        let q = self.live.sessions.get_mut(id).unwrap();
        q.range = true;
        q.from = TextEdit::new(&task.from);
        q.to = TextEdit::new(&task.to);
        q.caption = format!("({}, {}] / {}", task.from, task.to, task.component);
        q.next = None;
        self.capture_note_feed(id, false);
        self.apply_engine_result(id.clone(), Ok(report));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (State, String) {
        let mut s = State::demo();
        crate::inventory_view::scene(&mut s);
        let id = s
            .catalog
            .packages
            .iter()
            .find_map(|p| job(&s, &p.id).map(|_| p.id.clone()))
            .expect("fixture with a known source and confirmed update");
        (s, id)
    }

    #[test]
    fn prefetch_uses_confirmed_updates_but_not_offline_or_custom_selections() {
        let (mut s, id) = fixture();
        let expected = job(&s, &id).unwrap();
        assert!(Version::parse(&expected.to, releasescout_engine::Scheme::Numeric).is_ok());
        let q = s.live.sessions.get_mut(&id).unwrap();
        q.cache = 2;
        assert!(job(&s, &id).is_none());
        let q = s.live.sessions.get_mut(&id).unwrap();
        q.cache = 0;
        q.inventory_range = false;
        q.from = TextEdit::new("0.1.0");
        assert!(job(&s, &id).is_none());
        assert!(!s.note_prefetch.loading(&id));
    }

    #[test]
    fn prefetch_applies_without_changing_selection_and_never_replaces_a_manual_result() {
        let (mut s, id) = fixture();
        let selected = s.selected().unwrap().id.clone();
        let task = job(&s, &id).unwrap();
        let mut report = crate::ai_summary::tests::report();
        report.package = id.clone();
        report.stream = task.component.clone();
        let count = report.notes.len();
        s.apply_prefetched(task.clone(), report.clone());
        assert_eq!(s.selected().unwrap().id, selected);
        let q = &s.live.sessions[&id];
        assert_eq!(q.report.as_ref().unwrap().notes.len(), count);
        assert!(
            q.report_cache
                .as_ref()
                .unwrap()
                .matches_version_range(&task.from, &task.to, q)
        );
        report.notes.clear();
        s.apply_prefetched(task, report);
        assert_eq!(
            s.live.sessions[&id].report.as_ref().unwrap().notes.len(),
            count
        );
    }

    #[test]
    fn prefetch_scheduler_prioritizes_the_reader_and_drains_into_note_sessions() {
        fn response(task: Job, _: &Path) -> Outcome {
            let mut report = crate::ai_summary::tests::report();
            report.package = task.profile.id;
            report.stream = task.component;
            Outcome::Loaded(report)
        }

        let (mut s, id) = fixture();
        s.select_package(&id);
        s.note_prefetch.enabled = true;
        s.note_prefetch.refresh();
        assert!(s.poll_note_prefetch_with(response));
        assert!(s.note_prefetch.progress().active > 0);
        assert_eq!(
            s.note_prefetch.slots[0].active.as_ref().unwrap().profile.id,
            id
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while s.note_prefetch.loading(&id) && std::time::Instant::now() < deadline {
            s.poll_note_prefetch_with(response);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!s.note_prefetch.loading(&id));
        assert!(s.note_prefetch.progress().loaded > 0);
        assert!(s.live.sessions[&id].report.is_some());
        assert_eq!(s.selected().unwrap().id, id);
        assert!(s.live.active.is_none());
    }

    #[test]
    fn prefetch_progress_distinguishes_saved_ranges_from_missing_notes() {
        for (response, saved, unavailable) in [
            (
                (|_: Job, _: &Path| Outcome::Saved) as fn(Job, &Path) -> Outcome,
                1,
                0,
            ),
            (
                (|_: Job, _: &Path| Outcome::Unavailable) as fn(Job, &Path) -> Outcome,
                0,
                1,
            ),
        ] {
            let (mut s, id) = fixture();
            s.note_prefetch.enabled = true;
            s.note_prefetch.queue.push_back(id.clone());
            assert!(s.poll_note_prefetch_with(response));
            assert_eq!(s.note_prefetch.progress().active, 1);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while s.note_prefetch.loading(&id) && std::time::Instant::now() < deadline {
                s.poll_note_prefetch_with(response);
                std::thread::sleep(Duration::from_millis(1));
            }
            let progress = s.note_prefetch.progress();
            assert_eq!(
                (
                    progress.active,
                    progress.loaded,
                    progress.saved,
                    progress.unavailable
                ),
                (0, 0, saved, unavailable)
            );
            assert!(s.live.sessions[&id].report.is_none());
        }
    }

    #[test]
    fn retry_backoff_is_jittered_bounded_and_leaves_rate_limits_to_http_cache() {
        assert_eq!(retry_delay(0, 0), Duration::from_millis(500));
        assert_eq!(retry_delay(0, 499), Duration::from_millis(999));
        assert_eq!(retry_delay(1, 999), Duration::from_millis(1999));
        let http = |status| Error::Http {
            status,
            url: "https://example.test".into(),
            detail: String::new(),
        };
        assert!(retryable(&http(503)));
        assert!(retryable(&http(408)));
        assert!(!retryable(&http(404)));
        assert!(!retryable(&http(429)));
    }
}
