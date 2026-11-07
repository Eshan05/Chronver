use crate::model::{Release, State};
use releasescout_engine::{
    Basis, Catalog, Channel, Engine, Report, Selection, catalog,
    http::{CacheOptions, HttpClient},
};
use rune::{Latest, TextEdit};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub struct Query {
    pub restored: bool,
    pub pending_cache: Option<crate::note_cache::Context>,
    pub report_cache: Option<crate::note_cache::Context>,
    pub pending_feed: Option<String>,
    pub report_feed: Option<String>,
    pub extension_published: bool,
    pub inventory_range: bool,
    pub next: Option<releasescout_engine::HistoryCursor>,
    pub details: bool,
    pub component_open: bool,
    pub component: usize,
    pub range: bool,
    pub from: TextEdit,
    pub to: TextEdit,
    pub channel: usize,
    pub cache: usize,
    pub error: Option<String>,
    pub report: Option<Report>,
    pub caption: String,
    pub shown: usize,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            restored: false,
            pending_cache: None,
            report_cache: None,
            pending_feed: None,
            report_feed: None,
            extension_published: false,
            inventory_range: false,
            next: None,
            details: false,
            component_open: false,
            component: 0,
            range: false,
            from: TextEdit::new(""),
            to: TextEdit::new(""),
            channel: 0,
            cache: 0,
            error: None,
            report: None,
            caption: String::new(),
            shown: 50,
        }
    }
}
pub struct Live {
    pub history: Latest<(
        String,
        std::result::Result<releasescout_engine::HistoryPage, String>,
        bool,
    )>,
    pub catalog: Catalog,
    pub sessions: HashMap<String, Query>,
    pub work: Latest<(String, std::result::Result<Report, String>)>,
    pub active: Option<String>,
    pub settings_errors: Vec<String>,
}

fn default_channel(package: &releasescout_engine::Package) -> usize {
    package.streams.first().map_or(0, |s| match s.channel {
        Channel::Stable => 0,
        Channel::Preview => 1,
        Channel::All => 2,
    })
}

impl Live {
    pub fn new() -> Self {
        let catalog = catalog::bundled().expect("bundled engine catalog");
        let sessions = catalog
            .packages
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    Query {
                        channel: default_channel(p),
                        ..Default::default()
                    },
                )
            })
            .collect();
        Self {
            history: Latest::default(),
            catalog,
            sessions,
            work: Latest::default(),
            active: None,
            settings_errors: Vec::new(),
        }
    }
    pub fn load_local_settings(&mut self, directory: &Path) {
        self.settings_errors.clear();
        for (filename, load) in [
            (
                "catalog-overrides.json",
                catalog::apply_source_overrides
                    as fn(&mut Catalog, &Path) -> releasescout_engine::Result<()>,
            ),
            ("tracking.json", catalog::apply_tracking_preferences),
        ] {
            match load(&mut self.catalog, &directory.join(filename)) {
                Ok(()) => {}
                Err(releasescout_engine::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => self.settings_errors.push(format!("{filename}: {e}")),
            }
        }
        for p in &self.catalog.packages {
            if let Some(query) = self.sessions.get_mut(&p.id) {
                query.channel = default_channel(p);
            }
        }
    }
    pub fn unavailable(&self, id: &str) -> Option<String> {
        let p = self.catalog.packages.iter().find(|p| p.id == id)?;
        if !matches!(p.tracking.as_str(), "track" | "low_priority") {
            Some(format!("Tracking: {}", p.tracking.replace('_', " ")))
        } else if p.streams.is_empty() {
            Some("Source profile not configured".into())
        } else {
            None
        }
    }
}
pub fn cache_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("ReleaseScout/cache")
}

impl State {
    pub(crate) fn installed_update_range(&self, id: &str) -> Option<(&str, &str)> {
        let package = self.catalog.packages.iter().find(|p| p.id == id)?;
        let query = self.live.sessions.get(id)?;
        let profile = self.live.catalog.packages.iter().find(|p| p.id == id)?;
        if package.extension.is_some()
            || package.dependency.is_some()
            || package.watchlist.is_some()
            || profile.streams.get(query.component)?.basis != Basis::Version
        {
            return None;
        }
        let from = package.installed.as_deref()?.trim();
        let to = package.target.as_deref()?.trim();
        (!from.starts_with('<')
            && releasescout_engine::version::candidate_order(from, to).is_some_and(|ord| ord.is_gt()))
            .then_some((from, to))
    }
}
pub fn gcloud_scene(s: &mut State) {
    let page: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("artifacts/gcloud-history.json")
            .expect("generate artifacts/gcloud-history.json with the engine history command"),
    ).expect("valid saved history page");
    s.select_package("gcloud");
    s.engine_query().shown = 20;
    let report = Report {
        package:"gcloud".into(),stream:"main".into(),scope:"Google Cloud CLI".into(),
        basis:Basis::Version,status:"partial".into(),complete:false,
        notes:serde_json::from_value(page["report"]["notes"].clone()).expect("saved release notes"),
        warnings:vec!["Saved source history fixture".into()],evidence:vec![],attempts:vec![],
    };
    s.apply_engine_result("gcloud".into(), Ok(report));
    s.engine_query().next = serde_json::from_value(page["next"].clone()).expect("saved history cursor");
}

// Bound initial Markdown layout work, not fetched history or individual release bodies.
fn note_batch_end<'a>(bodies: impl Iterator<Item = &'a str>, start: usize, limit: usize) -> usize {
    let mut bytes = 0usize;
    let mut end = start;
    for body in bodies.skip(start).take(limit) {
        if end > start && bytes.saturating_add(body.len()) > 16 * 1024 { break; }
        bytes = bytes.saturating_add(body.len());
        end += 1;
    }
    end
}

impl State {
    pub fn show_more_notes(&mut self) {
        let Some(p) = self.selected() else { return; };
        let shown = self.live.sessions[&p.id].shown;
        let next = note_batch_end(p.releases.iter().map(|r|r.markdown.as_str()), shown, 50);
        self.engine_query().shown = next;
    }
    pub fn engine_query(&mut self) -> &mut Query {
        let id = self.selected().expect("selected package").id.clone();
        self.live
            .sessions
            .get_mut(&id)
            .expect("reference catalog ID")
    }
    pub fn fetch_notes(&mut self) {
        if self.selected().is_some_and(|p| p.watchlist.is_some()) {
            self.fetch_watchlist_notes();
            return;
        }
        if self.selected().is_some_and(|p| p.dependency.is_some()) {
            self.fetch_dependency_notes();
            return;
        }
        if self.selected().is_some_and(|p| p.extension.is_some()) {
            self.fetch_extension_notes();
            return;
        }
        let Some(id) = self.selected().map(|p| p.id.clone()) else {
            return;
        };
        self.capture_note_feed(&id, false);
        self.engine_query().inventory_range = false;
        if let Some(reason) = self.live.unavailable(&id) {
            self.engine_query().error = Some(reason);
            return;
        }
        if !self.live.sessions[&id].range {
            self.fetch_history(false);
            return;
        }
        self.live.history.cancel();
        let p = self
            .live
            .catalog
            .packages
            .iter()
            .find(|p| p.id == id)
            .unwrap();
        let q = &self.live.sessions[&id];
        let stream = &p.streams[q.component];
        let channel = [Channel::Stable, Channel::Preview, Channel::All][q.channel];
        let selection = if !q.range {
            Ok(Selection::Inspect)
        } else if stream.basis == Basis::Version {
            Ok(Selection::Versions {
                from: q.from.text.trim().into(),
                to: q.to.text.trim().into(),
                channel,
            })
        } else {
            q.from
                .text
                .trim()
                .parse()
                .and_then(|since| {
                    q.to.text
                        .trim()
                        .parse()
                        .map(|until| Selection::Dates { since, until })
                })
                .map_err(|_| "Use dates in YYYY-MM-DD format".to_string())
        };
        let selection = match selection {
            Ok(s) => s,
            Err(e) => {
                self.engine_query().error = Some(e);
                return;
            }
        };
        let caption = if q.range {
            format!(
                "({}, {}] / {}",
                q.from.text.trim(),
                q.to.text.trim(),
                stream.id
            )
        } else {
            format!(
                "Retrieved history / {} / installed range unknown",
                stream.id
            )
        };
        let component = stream.id.clone();
        let options = CacheOptions {
            directory: cache_directory(),
            public_only: true,
            offline: q.cache == 2,
            refresh: q.cache == 1,
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            ..Default::default()
        };
        let saved = self.fresh_note_lookup(&id);
        let document = saved
            .as_ref()
            .and_then(|_| crate::note_cache::single_document(stream));
        let catalog = self.live.catalog.clone();
        self.live.active = Some(id.clone());
        let q = self.live.sessions.get_mut(&id).unwrap();
        q.error = None;
        q.report = None;
        q.next = None;
        q.caption = caption;
        q.shown = 50;
        let p = self
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap();
        p.releases.clear();
        p.sample_notes = false;
        self.live.work.request(id, move |id| {
            let result = crate::note_cache::fetch_page(
                saved,
                document,
                || HttpClient::new(options),
                |fetcher| {
                    Engine { catalog, fetcher }
                        .notes(&id, Some(&component), selection, false)
                        .map(|report| releasescout_engine::HistoryPage { report, next: None })
                },
            )
            .map(|(page, _)| page.report)
            .map_err(|e| e.to_string());
            (id, result)
        });
    }
    pub fn poll_engine(&mut self) -> bool {
        let Some((id, result)) = self.live.work.take() else {
            return false;
        };
        if self.live.active.as_deref() != Some(&id) {
            return false;
        }
        self.apply_engine_result(id, result);
        true
    }
    pub fn fetch_history(&mut self, append: bool) {
        let Some(id) = self.selected().map(|p| p.id.clone()) else {
            return;
        };
        self.capture_note_feed(&id, append);
        if let Some(reason) = self.live.unavailable(&id) {
            self.engine_query().error = Some(reason);
            return;
        }
        let profile = self
            .live
            .catalog
            .packages
            .iter()
            .find(|p| p.id == id)
            .unwrap();
        let q = &self.live.sessions[&id];
        let component = profile.streams[q.component].id.clone();
        let cursor = if append {
            match &q.next {
                Some(c) if c.stream == component => Some(c.clone()),
                _ => return,
            }
        } else {
            None
        };
        let options = CacheOptions {
            directory: cache_directory(),
            public_only: true,
            offline: q.cache == 2,
            refresh: q.cache == 1,
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            ..Default::default()
        };
        let saved = if append {
            None
        } else {
            self.fresh_note_lookup(&id)
        };
        let document = saved
            .as_ref()
            .and_then(|_| crate::note_cache::single_document(&profile.streams[q.component]));
        let catalog = self.live.catalog.clone();
        self.live.work.cancel();
        self.live.active = Some(id.clone());
        let q = self.live.sessions.get_mut(&id).unwrap();
        q.error = None;
        if !append {
            q.report = None;
            q.next = None;
            q.shown = 20;
            q.caption = format!("Paged source history / {component} / installed range unknown");
            let p = self
                .catalog
                .packages
                .iter_mut()
                .find(|p| p.id == id)
                .unwrap();
            p.releases.clear();
            p.sample_notes = false;
        }
        self.live.history.request(id, move |id| {
            let result = crate::note_cache::fetch_page(
                saved,
                document,
                || HttpClient::new(options),
                |fetcher| Engine { catalog, fetcher }.history(&id, Some(&component), cursor, false),
            )
            .map(|(page, _)| page)
            .map_err(|e| e.to_string());
            (id, result, append)
        });
    }
    pub fn poll_history(&mut self) -> bool {
        let Some((id, result, append)) = self.live.history.take() else {
            return false;
        };
        if self.live.active.as_deref() != Some(&id) {
            return false;
        }
        self.live.active = None;
        match result {
            Ok(page) => {
                if append {
                    let q = &self.live.sessions[&id];
                    if let Some(feed) = q.pending_feed.as_ref().or(q.report_feed.as_ref()) {
                        let older: Vec<_> = page
                            .report
                            .notes
                            .iter()
                            .filter(|n| {
                                !q.report.as_ref().is_some_and(|old| {
                                    old.notes
                                        .iter()
                                        .any(|o| o.url == n.url && o.version == n.version)
                                })
                            })
                            .cloned()
                            .collect();
                        self.activity.dirty |= self.activity.data.acknowledge(feed, &older, false);
                    }
                }
                let q = self.live.sessions.get_mut(&id).unwrap();
                let mut report = page.report;
                if append && let Some(mut old) = q.report.take() {
                    for n in report.notes {
                        if !old.notes.iter().any(|o| {
                            o.url == n.url
                                && o.version == n.version
                                && o.body == n.body
                                && o.date == n.date
                        }) {
                            old.notes.push(n);
                        }
                    }
                    old.evidence.extend(report.evidence);
                    old.attempts.extend(report.attempts);
                    old.warnings.extend(report.warnings);
                    old.warnings
                        .retain(|w| !w.starts_with("More history available"));
                    old.warnings
                        .push("Paged snapshot; releases can shift between upstream pages".into());
                    old.warnings.sort();
                    old.warnings.dedup();
                    old.complete = false;
                    old.status = "partial".into();
                    report = old;
                }
                let shown = note_batch_end(report.notes.iter().map(|n|n.body.as_str()),
                    if append {q.shown} else {0}, 20);
                q.next = page.next;
                self.apply_engine_result(id.clone(), Ok(report));
                self.live.sessions.get_mut(&id).unwrap().shown = shown;
            }
            Err(e) => self.live.sessions.get_mut(&id).unwrap().error = Some(e),
        }
        true
    }
    pub fn apply_engine_result(&mut self, id: String, result: std::result::Result<Report, String>) {
        let result = match result {
            Ok(report) if report.status == "source_failed" && self.live.sessions[&id].restored
                && self.live.sessions[&id].report.is_some() => {
                Err("Source refresh failed; showing saved notes".into())
            }
            result => result,
        };
        let succeeded = result.is_ok();
        self.live.active = None;
        if self.live.sessions[&id].pending_feed.is_none() {
            self.capture_note_feed(&id, false);
        }
        if succeeded {
            self.ai.invalidate(&id);
        }
        let q = self.live.sessions.get_mut(&id).unwrap();
        match result {
            Ok(report) => {
                q.restored = crate::note_cache::is_saved(&report);
                if q.restored
                    && !q.caption.ends_with("Saved notes")
                    && !q.caption.ends_with(" / saved notes")
                {
                    q.caption.push_str(" / saved notes");
                }
                q.error = None;
                q.report_cache = q.pending_cache.take();
                q.shown = note_batch_end(report.notes.iter().map(|n|n.body.as_str()), 0, q.shown.max(1));
                q.report_feed = q.pending_feed.take();
                if let Some(feed) = &q.report_feed
                    && report.status != "source_failed"
                    && let Some(checked) = report
                        .evidence
                        .iter()
                        .filter_map(|e| {
                            chrono::DateTime::parse_from_rfc3339(&e.fetched_at)
                                .ok()
                                .map(|date| (date, &e.fetched_at))
                        })
                        .max_by_key(|(date, _)| *date)
                        .map(|(_, time)| time)
                {
                    self.activity.dirty |= crate::activity::record_check(
                        &mut self.activity.data.source_checks,
                        feed.clone(),
                        checked,
                    );
                }
                let p = self
                    .catalog
                    .packages
                    .iter_mut()
                    .find(|p| p.id == id)
                    .unwrap();
                p.releases = report
                    .notes
                    .iter()
                    .map(|n| Release {
                        version: if n.title.is_empty() {
                            n.version
                                .clone()
                                .unwrap_or_else(|| "Untitled release".into())
                        } else {
                            n.title.clone()
                        },
                        url: n.url.clone(),
                        published: n
                            .date
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| "Undated".into()),
                        markdown: n.body.clone(),
                    })
                    .collect();
                p.sample_notes = false;
                q.report = Some(report);
            }
            Err(error) => q.error = Some(error),
        }
        if crate::sidebar_filters::count(self) > 0 {
            self.refilter();
        } else if self
            .table
            .sort()
            .is_some_and(|(c, _)| matches!(c, "date" | "checked"))
        {
            self.sort_visible();
        }
        self.mark_current_notes_read();
        if succeeded {self.persist_notes(&id);}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_notes_are_revealed_in_bounded_batches_without_dropping_content() {
        let mut s = State::demo();
        s.select_package("gh");
        let mut data = report("gh");
        data.notes = (0..5).map(|i| {
            let mut n = data.notes[0].clone();
            n.title = format!("1.{i}.0");
            n.body = "Detailed authored change.\n".repeat(400);
            n
        }).collect();
        s.apply_engine_result("gh".into(),Ok(data));
        assert_eq!(s.live.sessions["gh"].shown,1);
        assert_eq!(s.selected().unwrap().releases.len(),5);
        s.show_more_notes();
        assert_eq!(s.live.sessions["gh"].shown,2);
        for _ in 0..5 {s.show_more_notes();}
        assert_eq!(s.live.sessions["gh"].shown,5);
        assert_eq!(note_batch_end(std::iter::once("x"),0,50),1);
        let large = "x".repeat(32*1024);
        assert_eq!(note_batch_end([large.as_str(),"next"].into_iter(),0,20),1);
        assert!(!s.live.work.is_pending());
    }
    #[test]
    #[ignore = "explicit saved Google Cloud history layout check; generate artifacts/gcloud-history.json"]
    fn cached_gcloud_notes_layout() {
        let mut s = State::demo();
        gcloud_scene(&mut s);
        let shown = s.live.sessions["gcloud"].shown;
        let title = s.live.sessions["gcloud"].report.as_ref().unwrap().notes[0].title.clone();
        assert!(shown < s.selected().unwrap().releases.len());
        let mut rt = rune::Runtime::with_text(s, crate::ui::view, (1280.,800.),
            rune::Env {system_dark:true, scale:1.},
            rune::text::TextSystem::new(crate::fonts::FACES,crate::fonts::FAMILIES));
        let started = std::time::Instant::now();
        rt.frame(1.);
        eprintln!("gcloud first frame {:?}, {} elements",started.elapsed(),rt.elements().len());
        assert!(rt.find("timeline-item-0").is_some());
        assert_eq!(rt.state.selected().unwrap().releases[0].version,title);
        assert!(rt.find("more-notes").is_some());
        assert!(rt.find(&format!("timeline-item-{shown}")).is_none());
        assert!(!rt.state.live.work.is_pending());
    }
    fn report(id: &str) -> Report {
        Report {
            package: id.into(),
            stream: "main".into(),
            scope: "Test report".into(),
            basis: Basis::Version,
            status: "partial".into(),
            complete: false,
            notes: vec![releasescout_engine::Note {
                title: "Test captured note".into(),
                version: Some("1.2.0".into()),
                date: None,
                url: "https://example.com/release".into(),
                body: "Fixed an example problem.".into(),
                authored_notes: true,
                preview: false,
            }],
            warnings: vec!["History is incomplete".into()],
            evidence: vec![],
            attempts: vec![],
        }
    }
    #[test]
    fn every_reference_has_a_profile_session_and_preferences_apply() {
        let mut s = State::demo();
        assert!(
            s.catalog
                .packages
                .iter()
                .all(|p| s.live.sessions.contains_key(&p.id))
        );
        assert!(!s.live.unavailable("easiercommit").unwrap().contains("excluded"));
        let settings = tempfile::tempdir().unwrap();
        std::fs::write(
            settings.path().join("tracking.json"),
            r#"{"schema_version":1,"products":[{"id":"easiercommit","tracking":"excluded"}]}"#,
        ).unwrap();
        std::fs::write(settings.path().join("catalog-overrides.json"), "{}").unwrap();
        s.live.load_local_settings(settings.path());
        assert_eq!(s.live.settings_errors.len(), 1);
        assert!(s.live.settings_errors[0].starts_with("catalog-overrides.json:"));
        assert!(
            s.live
                .unavailable("easiercommit")
                .unwrap()
                .contains("excluded")
        );
        assert!(
            s.live
                .unavailable("vsc")
                .unwrap()
                .contains("not configured")
        );
        assert_eq!(s.live.sessions["t3-code"].channel, 1);
    }
    #[test]
    fn older_page_merges_without_duplicates_or_losing_identity() {
        let mut s = State::demo();
        s.select_package("pnpm");
        s.apply_engine_result("gh".into(), Ok(report("gh")));
        let mut page = report("gh");
        let mut n = page.notes[0].clone();
        n.version = Some("1.1.0".into());
        n.url = "https://example.com/older".into();
        page.notes.push(n);
        s.live.active = Some("gh".into());
        s.select_package("pnpm");
        s.live.history.request((), move |_| {
            (
                "gh".into(),
                Ok(releasescout_engine::HistoryPage {
                    report: page,
                    next: None,
                }),
                true,
            )
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !s.poll_history() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            s.live.sessions["gh"].report.as_ref().unwrap().notes.len(),
            2
        );
        assert_eq!(s.selected().unwrap().id, "pnpm");
        assert!(s.live.sessions["gh"].next.is_none());
        let q = &s.live.sessions["gh"];
        assert_eq!(
            s.activity
                .data
                .unread(q.report_feed.as_ref().unwrap(), q.report.as_ref().unwrap())
                .len(),
            1,
            "Old pagination is not new, but must not dismiss the unread first page"
        );
    }
    #[test]
    fn failed_older_page_retains_previous_notes() {
        let mut s = State::demo();
        s.select_package("gh");
        s.apply_engine_result("gh".into(), Ok(report("gh")));
        s.live.active = Some("gh".into());
        s.live.history.request((), |_| {
            ("gh".into(), Err("temporary network error".into()), true)
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !s.poll_history() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            s.live.sessions["gh"].report.as_ref().unwrap().notes.len(),
            1
        );
        assert_eq!(s.selected().unwrap().releases.len(), 1);
        assert!(s.live.sessions["gh"].error.is_some());
    }
    #[test]
    fn completion_is_bound_to_requested_identity_not_current_selection() {
        let mut s = State::demo();
        s.select_package("pnpm");
        s.apply_engine_result("gh".into(), Ok(report("gh")));
        assert_eq!(s.selected().unwrap().id, "pnpm");
        let p = s.catalog.packages.iter().find(|p| p.id == "gh").unwrap();
        assert_eq!(p.releases[0].version, "Test captured note");
        assert!(p.installed.is_none());
        assert!(p.target.is_none());
        s.select_package("gh");
        assert!(s.selected_markdown().contains("History is incomplete"));
    }
    #[test]
    fn independent_query_inputs_and_failure_states() {
        let mut s = State::demo();
        s.select_package("gh");
        s.engine_query().from = TextEdit::new("1.0.0");
        s.select_package("pnpm");
        assert!(s.engine_query().from.text.is_empty());
        s.apply_engine_result("gh".into(), Err("offline cache miss".into()));
        assert_eq!(
            s.live.sessions["gh"].error.as_deref(),
            Some("offline cache miss")
        );
    }
    #[test]
    fn background_result_does_not_block_or_cross_selection() {
        let mut s = State::demo();
        s.live.active = Some("gh".into());
        s.live.work.request((), |_| ("gh".into(), Ok(report("gh"))));
        s.select_package("pnpm");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !(s.poll_engine() | s.poll_history()) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(s.live.sessions["gh"].report.is_some());
        assert_eq!(s.selected().unwrap().id, "pnpm");
    }
    #[test]
    fn offline_fetch_runs_real_engine_without_touching_inventory_versions() {
        let mut s = State::demo();
        s.select_package("gh");
        s.engine_query().cache = 2;
        let profile = s
            .live
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.id == "gh")
            .unwrap();
        profile.streams[0].sources = vec![releasescout_engine::Source::Document {
            url: "https://releasescout-offline-test.invalid/feed".into(),
            format: releasescout_engine::Format::Markdown,
            boundary: r"(?P<version>\d+\.\d+\.\d+)".into(),
            selector: None,
            exclude: None,
            date_formats: vec![],
            paragraphs: false,
        }];
        s.fetch_notes();
        assert!(s.live.active.is_some());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !(s.poll_engine() | s.poll_history()) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let q = &s.live.sessions["gh"];
        assert!(q.error.as_ref().unwrap().contains("offline cache miss"));
        assert!(s.selected().unwrap().releases.is_empty());
        assert!(s.selected().unwrap().installed.is_none());
    }
    #[test]
    #[ignore = "explicit public-network integration check"]
    fn live_github_fetch_reaches_the_rune_model() {
        let mut s = State::demo();
        s.select_package("gh");
        s.fetch_notes();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while !(s.poll_engine() | s.poll_history()) {
            assert!(std::time::Instant::now() < deadline, "fetch did not settle");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let q = &s.live.sessions["gh"];
        assert!(q.error.is_none(), "{:?}", q.error);
        let report = q.report.as_ref().unwrap();
        assert!(report.notes.len() <= 20);
        assert!(
            q.next.is_some(),
            "GitHub CLI history should have another page"
        );
        assert!(
            report.notes.iter().any(|n| n.authored_notes),
            "{:?}",
            report.attempts
        );
        assert!(!s.selected().unwrap().sample_notes);
        assert!(s.selected().unwrap().installed.is_none());
        assert_eq!(s.selected().unwrap().releases.len(), report.notes.len());
        assert!(
            s.selected_markdown()
                .contains("Source: https://github.com/cli/cli/releases/")
        );
    }
}
