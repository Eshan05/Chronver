use crate::{
    background_feed::{Feed, FeedRelease, Subject, note_excerpt},
    background_watch::{self, Follow, Results, Settings, Target},
    background_windows,
    model::{LibrarySection, Package, State},
};
use releasescout_engine::{
    Basis, Channel,
    watchlist::{self, Ecosystem, Entry, PublishedRelease},
};
use rune::{El, Latest, ScrollSpec, col, icon, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Dialog, IconButton, IconButtonVariant, Kit, SegmentedControl,
    glyphs, scroll_bar,
};
use std::time::{Duration, Instant};

type Snapshot = (Settings, Results);
type Releases = Result<Vec<PublishedRelease>, String>;
const HISTORY_TTL: Duration = Duration::from_secs(300);
const FEED: usize = 0;
const WATCHED: usize = 1;
const UPDATES: usize = 2;

struct NpmHistory {
    name: String,
    expanded: bool,
    requested_at: Instant,
    outcome: Option<Releases>,
}

#[derive(Default)]
pub struct Watch {
    open: bool,
    workspace: bool,
    restored_settings: Option<Settings>,
    tab: usize,
    snapshot: Option<Snapshot>,
    error: Option<String>,
    activation: Option<String>,
    npm_history: Option<NpmHistory>,
    pub feed: Feed,
    pub history_work: Latest<Releases>,
    pub work: Latest<Result<Snapshot, String>>,
    pub preference: Latest<Settings>,
}

impl Watch {
    fn settings(&self) -> Option<&Settings> {
        self.snapshot
            .as_ref()
            .map(|(settings, _)| settings)
            .or(self.restored_settings.as_ref())
    }

    fn follows(&self) -> &[Follow] {
        self.settings()
            .map_or(&[], |settings| settings.items.as_slice())
    }

    pub fn enabled(&self) -> bool {
        self.settings().is_some_and(|settings| settings.enabled)
    }

    pub fn in_workspace(&self) -> bool {
        self.workspace && self.enabled()
    }

    pub fn leave_workspace(&mut self) {
        self.workspace = false;
    }

    fn set_snapshot(&mut self, snapshot: Snapshot) {
        if !snapshot.0.enabled {
            self.leave_workspace();
        }
        self.restored_settings = None;
        self.snapshot = Some(snapshot);
    }
}

fn follow(s: &State, p: &Package) -> Option<Follow> {
    let target = if let Some(info) = &p.extension {
        Target::Extension {
            editor: p.provider?,
            id: info.id.clone(),
            profile: info.profile_id.clone()?,
        }
    } else if let Some(entry) = &p.watchlist {
        if entry.ecosystem != releasescout_engine::watchlist::Ecosystem::Npm {
            return None;
        }
        Target::Npm {
            name: entry.name.clone(),
        }
    } else {
        let product = p.reference_id.as_deref()?;
        let package = s
            .live
            .catalog
            .packages
            .iter()
            .find(|profile| profile.id == p.id)?;
        let component = s.live.sessions.get(&p.id).map_or(0, |q| q.component);
        let stream = package.streams.get(component)?;
        if stream.basis != Basis::Version || stream.channel != Channel::Stable {
            return None;
        }
        let reviewed = s.live.catalog.packages.iter().find(|p| p.id == product)?;
        if !reviewed
            .streams
            .iter()
            .any(|candidate| candidate.id == stream.id)
        {
            return None;
        }
        Target::App {
            package: product.to_owned(),
            stream: stream.id.clone(),
        }
    };
    let title = if p.extension.is_some() {
        format!(
            "{} / {} / {}",
            s.display_name(p),
            p.provider?.editor_name()?,
            crate::extension_ui::profile_name(p)
        )
    } else {
        s.display_name(p).to_owned()
    };
    Some(Follow {
        target,
        title: title.chars().take(180).collect(),
    })
}

pub fn reader_button(s: &State, p: &Package, k: &Kit) -> Option<El> {
    if p.watchlist.is_some() {
        return None;
    }
    let item = follow(s, p)?;
    let watch = &s.background_watch;
    let following = watch.follows().iter().any(|saved| saved.target == item.target);
    let action = if following { "Unwatch" } else { "Watch" };
    Some(
        IconButton::new(&crate::app_chrome::BELL)
            .variant(if following {
                IconButtonVariant::Contrast
            } else {
                IconButtonVariant::Ghost
            })
            .control_size(ControlSize::Large)
            .label(format!("{action} {}", s.display_name(p)))
            .disabled(watch.settings().is_none() || watch.work.is_pending())
            .on_press(move |s: &mut State| s.watch_request(Action::Toggle(item.clone())))
            .build(k)
            .key("reader-watch"),
    )
}

enum Action {
    Read,
    Check,
    SetEnabled(bool),
    Toggle(Follow),
    Remove(String),
}

fn finish(action: Action) -> releasescout_engine::Result<Snapshot> {
    let dir = background_watch::directory();
    let mut cfg = background_watch::settings(&dir)?;
    match action {
        Action::Read => {}
        Action::Check => {
            background_watch::run(&dir, true)?;
        }
        Action::SetEnabled(enabled) => {
            cfg.enabled = enabled;
            if enabled {
                background_windows::schedule(true)?;
                if let Err(error) = cfg.store(&dir) {
                    let _ = background_windows::schedule(false);
                    return Err(error);
                }
            } else {
                cfg.store(&dir)?;
                background_windows::schedule(false)?;
            }
        }
        Action::Toggle(item) => {
            let old_len = cfg.items.len();
            cfg.items.retain(|existing| existing.target != item.target);
            if old_len == cfg.items.len() {
                cfg.items.push(item);
            }
            cfg.store(&dir)?;
        }
        Action::Remove(key) => {
            cfg.items.retain(|item| item.target.key() != key);
            cfg.store(&dir)?;
        }
    }
    Ok((cfg, background_watch::results(&dir)?))
}

impl State {
    fn load_npm_history(&mut self, entry: Entry) {
        let outcome = self.background_watch.feed.releases_for(&entry.name).map(Ok);
        self.background_watch.history_work.cancel();
        let needs_fetch = outcome.is_none();
        self.background_watch.npm_history = Some(NpmHistory {
            name: entry.name.clone(),
            expanded: true,
            requested_at: Instant::now(),
            outcome,
        });
        if needs_fetch {
            self.background_watch.history_work.request(entry, |entry| {
                watchlist::recent_npm_releases(&entry).map_err(|error| error.to_string())
            });
        }
    }

    fn toggle_npm_history(&mut self, entry: &Entry) {
        if let Some(history) = &mut self.background_watch.npm_history
            && history.name == entry.name
            && (history.expanded || history.requested_at.elapsed() < HISTORY_TTL)
        {
            history.expanded = !history.expanded;
        } else {
            self.load_npm_history(entry.clone());
        }
    }

    pub fn restore_background_watch(&self) {
        self.background_watch.preference.request((), |_| {
            background_watch::settings(&background_watch::directory()).unwrap_or_default()
        });
    }

    fn watch_request(&mut self, action: Action) {
        if self.background_watch.work.is_pending() {
            return;
        }
        self.background_watch.error = None;
        self.background_watch
            .work
            .request((), move |_| finish(action).map_err(|e| e.to_string()));
    }

    pub fn open_background_watch(&mut self) {
        if !self.background_watch.in_workspace() || self.inventory.open {
            self.background_watch.open = true;
            if self.background_watch.tab == FEED {
                self.background_watch.tab = WATCHED;
            }
        }
        self.watch_request(Action::Read);
    }

    pub fn open_background_watch_settings(&mut self) {
        self.background_watch.tab = WATCHED;
        self.open_background_watch();
    }

    pub fn open_watch_workspace(&mut self) {
        if self.background_watch.enabled() {
            self.inventory.open = false;
            self.background_watch.open = false;
            self.background_watch.workspace = true;
            self.background_watch.tab = FEED;
            self.watch_request(Action::Read);
            self.poll_watch_feed();
        }
    }

    pub fn activate_background_watch(&mut self, key: String) {
        self.background_watch.activation = (key != "all").then_some(key);
        self.background_watch.tab = UPDATES;
        self.open_background_watch();
    }

    pub fn poll_background_watch(&mut self) -> bool {
        let feed_changed = self.poll_watch_feed();
        let history_changed = if let Some(outcome) = self.background_watch.history_work.take()
            && let Some(history) = self.background_watch.npm_history.as_mut()
        {
            history.outcome = Some(outcome);
            true
        } else {
            false
        };
        let restored = if let Some(settings) = self.background_watch.preference.take()
            && self.background_watch.snapshot.is_none()
        {
            self.background_watch.restored_settings = Some(settings);
            true
        } else {
            false
        };
        let Some(outcome) = self.background_watch.work.take() else {
            return restored || history_changed || feed_changed;
        };
        match outcome {
            Ok(snapshot) => {
                let target = self.background_watch.activation.take().and_then(|key| {
                    snapshot
                        .1
                        .recent
                        .iter()
                        .find(|event| event.item.target.key() == key)
                        .map(|e| e.item.target.clone())
                });
                self.background_watch.set_snapshot(snapshot);
                self.poll_watch_feed();
                if let Some(target) = target
                    && !target.is_chronver()
                {
                    self.open_watched_target(&target);
                }
            }
            Err(error) => {
                if !self.background_watch.open && !self.background_watch.in_workspace() {
                    self.status = format!("Could not change Watch: {error}");
                }
                self.background_watch.error = Some(error);
            }
        }
        true
    }

    fn poll_watch_feed(&mut self) -> bool {
        let watch = &mut self.background_watch;
        let active =
            watch.snapshot.is_some() && watch.tab == FEED && (watch.in_workspace() || watch.open);
        let follows = watch
            .snapshot
            .as_ref()
            .map_or(&[][..], |(settings, _)| &settings.items);
        watch.feed.poll(
            active,
            &self.packages.watch.entries,
            follows,
            &self.live.catalog,
        )
    }

    pub(crate) fn watch_icon_packages(&self) -> Vec<usize> {
        let watch = &self.background_watch;
        if !watch.in_workspace() && !watch.open {
            return Vec::new();
        }
        let mut software = std::collections::HashSet::new();
        let names: std::collections::HashSet<_> = if watch.tab == FEED {
            let mut names = std::collections::HashSet::new();
            for (subject, _) in watch
                .feed
                .cards(&self.packages.watch.entries, watch.follows())
                .into_iter()
                .take(watch.feed.limit())
            {
                match subject {
                    Subject::Npm(entry) => {
                        names.insert(entry.name.as_str());
                    }
                    Subject::App(Follow {
                        target: Target::App { package, .. },
                        ..
                    }) => {
                        software.insert(package.as_str());
                    }
                    Subject::App(_) => {}
                }
            }
            names
        } else if watch.tab == WATCHED {
            self.packages
                .watch
                .entries
                .iter()
                .filter(|entry| entry.ecosystem == Ecosystem::Npm)
                .take(16)
                .map(|entry| entry.name.as_str())
                .collect()
        } else {
            return Vec::new();
        };
        self.catalog
            .packages
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.reference_id
                    .as_deref()
                    .is_some_and(|id| software.contains(id))
                    || software.contains(p.id.as_str())
                    || p.watchlist.as_ref().is_some_and(|entry| {
                        entry.ecosystem == Ecosystem::Npm && names.contains(entry.name.as_str())
                    })
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn open_watched_notes(&mut self, target: &Target, tx: &mut rune::Tx) {
        if target.is_chronver() {
            self.status = match tx.open_url(background_watch::CHRONVER_RELEASES) {
                Ok(()) => "Opened Chronver releases".into(),
                Err(error) => format!("Could not open Chronver releases: {error}"),
            };
        } else {
            self.open_watched_target(target);
        }
    }

    fn open_watched_target(&mut self, target: &Target) {
        let selected = self
            .catalog
            .packages
            .iter()
            .find(|p| match target {
                Target::App { package, stream } => {
                    p.reference_id.as_ref() == Some(package)
                        && self
                            .live
                            .catalog
                            .packages
                            .iter()
                            .find(|profile| profile.id == p.id)
                            .is_some_and(|profile| {
                                profile
                                    .streams
                                    .iter()
                                    .any(|candidate| candidate.id == *stream)
                            })
                }
                Target::Extension {
                    editor,
                    id,
                    profile,
                } => {
                    p.provider == Some(*editor)
                        && p.extension.as_ref().is_some_and(|info| {
                            info.id.eq_ignore_ascii_case(id)
                                && info.profile_id.as_ref() == Some(profile)
                        })
                }
                Target::Npm { name } => p
                    .watchlist
                    .as_ref()
                    .is_some_and(|entry| entry.name == *name),
            })
            .map(|p| p.id.clone());
        let Some(id) = selected else {
            self.background_watch.error = Some("This item is no longer in the library".into());
            return;
        };
        let section = match target {
            Target::App { .. } => LibrarySection::Software,
            Target::Extension { .. } => {
                if self.dataset != crate::model::Dataset::Detected {
                    self.set_dataset(crate::model::Dataset::Detected);
                }
                LibrarySection::Extensions
            }
            Target::Npm { .. } => {
                self.packages.watch.project_mode = false;
                LibrarySection::Packages
            }
        };
        self.set_section(section);
        let visible = crate::sidebar_groups::shown(self).iter().any(|&i| {
            self.rows[i]
                .members
                .iter()
                .any(|&j| self.catalog.packages[j].id == id)
        });
        if !visible {
            self.command.revealed = Some(id.clone());
            self.set_dataset(self.dataset);
        }
        self.select_package(&id);
        if !self.selected().is_some_and(|p| p.id == id) {
            self.background_watch.error =
                Some("This item is hidden by the current library view".into());
            return;
        }
        if let Target::App { stream, .. } = target
            && let Some(index) = self
                .live
                .catalog
                .packages
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| p.streams.iter().position(|s| s.id == *stream))
            && let Some(query) = self.live.sessions.get_mut(&id)
        {
            if query.component != index {
                query.from = rune::TextEdit::new("");
                query.to = rune::TextEdit::new("");
                query.inventory_range = false;
            }
            query.component = index;
            query.channel = 0;
        }
        self.inventory.open = false;
        self.pane.set_open(true);
        self.background_watch.leave_workspace();
        self.background_watch.open = false;
    }
}

#[cfg(test)]
pub fn scene(s: &mut State) {
    crate::watchlist_ui::scene(s);
    let name = s
        .selected()
        .unwrap()
        .watchlist
        .as_ref()
        .unwrap()
        .name
        .clone();
    let mut results = Results::default();
    results.recent.push(background_watch::Event {
        item: Follow {
            title: name.clone(),
            target: Target::Npm { name },
        },
        version: "2.0.0".into(),
        found_at: chrono::Utc::now(),
    });
    s.background_watch.snapshot = Some((Settings::default(), results));
    s.background_watch.open = true;
    s.background_watch.tab = WATCHED;
}

#[cfg(test)]
pub fn set_fixture_enabled(s: &mut State, enabled: bool) {
    let (_, results) = s.background_watch.snapshot.take().unwrap_or_default();
    s.background_watch.set_snapshot((
        Settings {
            enabled,
            ..Settings::default()
        },
        results,
    ));
}

#[cfg(test)]
pub fn npm_history_scene(s: &mut State) -> Entry {
    scene(s);
    let entry = s.selected().unwrap().watchlist.clone().unwrap();
    let key = Target::Npm {
        name: entry.name.clone(),
    }
    .key();
    let (_, results) = s.background_watch.snapshot.as_mut().unwrap();
    *results = serde_json::from_value(serde_json::json!({
        "verified": 1, "unverified": 0, "recent": [],
        "baselines": {(key): "15.1.0"}
    }))
    .unwrap();
    s.background_watch.npm_history = Some(NpmHistory {
        name: entry.name.clone(),
        expanded: false,
        requested_at: Instant::now(),
        outcome: Some(Ok(vec![PublishedRelease {
            version: "15.0.1".into(),
            published_at: "2026-10-03T00:00:00Z".parse().unwrap(),
        }])),
    });
    entry
}

#[cfg(test)]
pub fn npm_feed_scene(s: &mut State) {
    use watchlist::Group;
    s.packages.watch.entries = ["react", "zod", "next", "typescript", "vitest"]
        .into_iter()
        .map(|name| Entry {
            name: name.into(),
            ecosystem: Ecosystem::Npm,
            group: Group::Core,
        })
        .collect();
    s.sync_watchlist();
    set_fixture_enabled(s, true);
    s.background_watch.workspace = true;
    let histories: &[(&str, &str, &[(&str, &str)])] = &[
        (
            "react",
            "A library for building user interfaces.",
            &[
                ("19.0.1", "2026-10-09T01:00:00Z"),
                ("19.0.0", "2026-09-10T01:00:00Z"),
            ],
        ),
        (
            "zod",
            "TypeScript-first schema validation.",
            &[
                ("4.6.7", "2026-10-12T01:00:00Z"),
                ("4.6.6", "2026-10-11T01:00:00Z"),
                ("4.6.5", "2026-10-10T08:00:00Z"),
                ("4.6.4", "2026-10-10T02:00:00Z"),
                ("4.6.3", "2026-10-09T23:00:00Z"),
            ],
        ),
        (
            "next",
            "React framework.",
            &[("16.0.1", "2026-10-08T01:00:00Z")],
        ),
        (
            "typescript",
            "Typed JavaScript.",
            &[("6.0.1", "2026-10-07T01:00:00Z")],
        ),
        (
            "vitest",
            "A JavaScript test runner.",
            &[("5.0.1", "2026-10-06T01:00:00Z")],
        ),
    ];
    for &(package, description, versions) in histories {
        s.background_watch.feed.fixture(
            package,
            description,
            versions
                .iter()
                .map(|&(version, date)| PublishedRelease {
                    version: version.into(),
                    published_at: date.parse().unwrap(),
                })
                .collect(),
        );
    }
    let id = s.packages.watch.entries[1].id();
    s.live.sessions.get_mut(&id).unwrap().report = Some(releasescout_engine::Report {
        package: id.clone(),
        stream: "zod".into(),
        scope: "Zod releases".into(),
        basis: Basis::Version,
        status: "ok".into(),
        complete: false,
        notes: vec![releasescout_engine::Note {
            title: "Zod 4.6.7".into(),
            version: Some("4.6.7".into()),
            date: None,
            url: "https://example.com/release".into(),
            body: "Improved **schema validation**.".into(),
            authored_notes: true,
            preview: false,
        }],
        warnings: vec![],
        evidence: vec![],
        attempts: vec![],
    });
}

#[cfg(test)]
pub fn software_feed_scene(s: &mut State) {
    npm_feed_scene(s);
    let app = Follow {
        target: Target::App {
            package: "atlas-cli".into(),
            stream: "main".into(),
        },
        title: "MongoDB Atlas CLI".into(),
    };
    s.background_watch.feed.fixture_app(
        &app,
        vec![
            FeedRelease {
                version: "1.60.1".into(),
                published_at: Some("2026-10-20T00:00:00Z".parse().unwrap()),
                excerpt: Some("Added platform support.".into()),
            },
            FeedRelease {
                version: "1.60.0".into(),
                published_at: None,
                excerpt: None,
            },
        ],
    );
    s.background_watch
        .snapshot
        .as_mut()
        .unwrap()
        .0
        .items
        .push(app);
}

fn line(value: impl Into<String>, k: &Kit) -> El {
    text(value.into())
        .font(k.ty.text_xsmall)
        .color(k.c.text_muted)
        .min_w(0.)
        .wrap_anywhere()
}

fn npm_icon(s: &State, k: &Kit, entry: &Entry, size: f32) -> El {
    if let Some(package) = s
        .catalog
        .packages
        .iter()
        .find(|p| p.watchlist.as_ref() == Some(entry))
    {
        crate::ui::mark(package, k, size)
    } else {
        icon(&glyphs::FILE_CODE, size)
            .color(k.c.icon_muted)
            .a11y_hidden()
    }
}

fn card_blurb(s: &State, entry: &Entry, release: &FeedRelease) -> (&'static str, String) {
    let note = s
        .live
        .sessions
        .get(&entry.id())
        .and_then(|query| query.report.as_ref())
        .and_then(|report| {
            report.notes.iter().find(|note| {
                note.authored_notes && note.version.as_deref() == Some(release.version.as_str())
            })
        });
    if let Some(note) = note {
        let excerpt = note_excerpt(&note.body);
        if !excerpt.is_empty() {
            return ("Release notes", excerpt);
        }
    }
    if let Some(description) = s.background_watch.feed.description(&entry.name) {
        return ("Package overview", description.to_owned());
    }
    ("", "No preview available".into())
}

fn release_card(
    s: &State,
    k: &Kit,
    subject: Subject<'_>,
    release: &FeedRelease,
    earlier: &[FeedRelease],
) -> El {
    let title = subject.title();
    let (key, target, source, mark, blurb) = match subject {
        Subject::Npm(entry) => {
            let group = match entry.group {
                watchlist::Group::Core => "Core",
                watchlist::Group::Tooling => "Tooling",
            };
            (
                entry.id(),
                Target::Npm {
                    name: entry.name.clone(),
                },
                format!("npm / {group}"),
                npm_icon(s, k, entry, 34.),
                card_blurb(s, entry, release),
            )
        }
        Subject::App(follow) => {
            let package = match &follow.target {
                Target::App { package, .. } => package.as_str(),
                _ => unreachable!("feed app subjects are filtered"),
            };
            let model = s
                .catalog
                .packages
                .iter()
                .find(|p| p.reference_id.as_deref() == Some(package) || p.id == package);
            let mark = if follow.target.is_chronver() {
                crate::app_chrome::brand_mark(34.)
            } else {
                model
                    .map(|p| crate::ui::mark(p, k, 34.))
                    .unwrap_or_else(|| {
                        icon(&glyphs::FILE_CODE, 34.)
                            .color(k.c.icon_muted)
                            .a11y_hidden()
                    })
            };
            let blurb = if let Some(excerpt) = &release.excerpt {
                ("Release notes", excerpt.clone())
            } else if let Some(details) = model.and_then(|p| p.description.as_deref()) {
                ("Software overview", note_excerpt(details))
            } else {
                ("", "No preview available".into())
            };
            (
                format!("app-{package}"),
                follow.target.clone(),
                "Software".into(),
                mark,
                blurb,
            )
        }
    };
    let date = release_date(release);
    let (label, preview) = blurb;
    col()
        .key(format!("feed-{key}"))
        .min_w(0.)
        .h(306.)
        .px(14.)
        .py(12.)
        .gap(8.)
        .radius(8.)
        .fill(k.c.layer_01)
        .border(1., k.c.border_muted)
        .child(
            row()
                .center_y()
                .gap(9.)
                .child(mark.key("feed-package-icon"))
                .child(line(source, k).grow(1.))
                .child(line(date, k))
                .child(
                    crate::sidebar_actions::action(
                        "feed-notes",
                        &glyphs::ARROW_RIGHT,
                        &format!("Open {title} notes"),
                        k,
                    )
                    .on_press_tx(move |s: &mut State, tx| s.open_watched_notes(&target, tx)),
                ),
        )
        .child(
            text(format!("{title} {}", release.version))
                .font(rune::TextStyle::new(18., 600.).lh(22.))
                .color(k.c.text_base)
                .w_full()
                .min_w(0.)
                .wrap_anywhere()
                .max_lines(2),
        )
        .child(
            col()
                .grow(1.)
                .min_h(0.)
                .gap(3.)
                .children((!label.is_empty()).then(|| line(label, k)))
                .child(
                    text(preview)
                        .font(k.ty.text_small.lh(18.))
                        .color(k.c.text_muted)
                        .w_full()
                        .min_w(0.)
                        .wrap_anywhere()
                        .max_lines(3),
                ),
        )
        .child(earlier_releases(earlier, k))
}

fn release_date(release: &FeedRelease) -> String {
    release.published_at.map_or_else(
        || "Undated".into(),
        |date| date.format("%b %d, %Y").to_string(),
    )
}

fn earlier_releases(releases: &[FeedRelease], k: &Kit) -> El {
    let mut history = col().key("earlier-releases").h(96.).gap(3.);
    if !releases.is_empty() {
        history = history
            .child(rune::el().h(1.).w_full().fill(k.c.border_muted))
            .child(line("Earlier releases", k));
        for release in releases {
            history = history.child(
                row()
                    .center_y()
                    .gap(8.)
                    .h(16.)
                    .child(
                        text(&release.version)
                            .font(k.ty.caption().mono())
                            .color(k.c.text_base)
                            .grow(1.)
                            .min_w(0.)
                            .truncate(),
                    )
                    .child(
                        text(release_date(release))
                            .font(k.ty.caption())
                            .color(k.c.text_muted),
                    ),
            );
        }
    }
    history
}

fn feed_entries(s: &State, k: &Kit, columns: usize) -> El {
    let feed = &s.background_watch.feed;
    let watched = &s.packages.watch.entries;
    let follows = s.background_watch.follows();
    let cards = feed.cards(watched, follows);
    let busy = feed.updating();
    let has_items = follows
        .iter()
        .any(|f| matches!(f.target, Target::App { .. }))
        || watched
            .iter()
            .any(|entry| entry.ecosystem == Ecosystem::Npm);
    let failures = feed.unavailable(watched, follows);
    let mut body = col().w_full().gap(8.).child(
        row()
            .gap(8.)
            .center_y()
            .child(
                text("Recent releases")
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .grow(1.),
            )
            .child(
                Button::new("Refresh feed")
                    .variant(ButtonVariant::Outline)
                    .disabled(busy || !has_items)
                    .on_press(|s: &mut State| {
                        s.background_watch.feed.refresh();
                        s.poll_watch_feed();
                    })
                    .build(k)
                    .key("refresh-watch-feed"),
            ),
    );
    if busy {
        body = body.child(line("Checking releases...", k));
    }
    if failures > 0 {
        body = body.child(line(
            format!("Release history unavailable for {failures} watched items"),
            k,
        ));
    }
    if cards.is_empty() && !busy {
        body = body.child(line(
            if has_items {
                "No stable releases found"
            } else {
                "No software or npm packages watched"
            },
            k,
        ));
    }
    body = body.child(
        col()
            .key("watch-feed-grid")
            .w_full()
            .gap(12.)
            .equal_columns(columns)
            .children(
                cards
                    .iter()
                    .take(feed.limit())
                    .filter_map(|(subject, releases)| {
                        let (newest, earlier) = releases.split_first()?;
                        Some(release_card(s, k, *subject, newest, earlier))
                    }),
            ),
    );
    if cards.len() > feed.limit() {
        body = body.child(
            Button::new("Show more")
                .on_press(|s: &mut State| s.background_watch.feed.show_more())
                .build(k),
        );
    }
    body
}

fn npm_row(s: &State, k: &Kit, entry: &Entry, results: &Results) -> El {
    let target = Target::Npm {
        name: entry.name.clone(),
    };
    let verified = results.last_verified(&target).unwrap_or("Not verified yet");
    let history = s
        .background_watch
        .npm_history
        .as_ref()
        .filter(|history| history.expanded && history.name == entry.name);
    let id = entry.id();
    let selected = entry.clone();
    let mut item = col().w_full().gap(4.).key(format!("npm-{id}")).child(
        row()
            .key("release-history")
            .w_full()
            .h(30.)
            .px(8.)
            .gap(8.)
            .center_y()
            .radius(6.)
            .role("button")
            .label(&entry.name)
            .description(format!("Last verified: {verified}"))
            .expanded(history.is_some())
            .focusable(true)
            .focus_ring(1., -1., k.c.border_focus)
            .hover(k.c.hover)
            .on_press(move |s: &mut State| s.toggle_npm_history(&selected))
            .child(
                icon(
                    if history.is_some() {
                        &glyphs::CHEVRON_UP
                    } else {
                        &glyphs::CHEVRON_DOWN
                    },
                    13.,
                )
                .color(k.c.icon_muted)
                .a11y_hidden(),
            )
            .child(npm_icon(s, k, entry, 18.))
            .child(
                text(&entry.name)
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .grow(1.)
                    .min_w(0.)
                    .truncate()
                    .a11y_hidden(),
            )
            .child(
                text(verified)
                    .font(k.ty.text_xsmall.mono())
                    .color(k.c.text_muted)
                    .max_w(155.)
                    .min_w(0.)
                    .truncate()
                    .a11y_hidden(),
            ),
    );
    if let Some(history) = history {
        let mut detail = col().key("releases").w_full().gap(6.).px(28.).pb(8.);
        match &history.outcome {
            None => detail = detail.child(line("Loading releases...", k)),
            Some(Err(error)) => {
                let retry = entry.clone();
                detail = detail.child(line(error, k)).child(
                    Button::new("Retry")
                        .on_press(move |s: &mut State| s.load_npm_history(retry.clone()))
                        .build(k),
                );
            }
            Some(Ok(releases)) if releases.is_empty() => {
                detail = detail.child(line("No dated stable releases", k));
            }
            Some(Ok(releases)) => {
                detail = detail.child(line("Recent stable releases", k));
                for release in releases {
                    detail = detail.child(
                        row()
                            .gap(8.)
                            .center_y()
                            .child(
                                text(&release.version)
                                    .font(k.ty.text_xsmall.mono())
                                    .color(k.c.text_base)
                                    .grow(1.)
                                    .min_w(0.)
                                    .truncate(),
                            )
                            .child(line(
                                release.published_at.format("%b %d, %Y").to_string(),
                                k,
                            )),
                    );
                }
            }
        }
        item = item.child(
            detail.child(
                Button::new("Open notes")
                    .variant(ButtonVariant::Outline)
                    .on_press(move |s: &mut State| s.open_watched_target(&target))
                    .build(k),
            ),
        );
    }
    item
}

fn chronver_watch_row(cfg: &Settings, results: &Results, busy: bool, k: &Kit) -> El {
    let target = Target::chronver();
    let following = cfg.items.iter().any(|item| item.target == target);
    let status = if following {
        results.last_verified(&target).map_or_else(
            || "No verified public release yet".into(),
            |version| format!("Last verified: {version}"),
        )
    } else {
        "Public releases".into()
    };
    row()
        .key("chronver-watch")
        .gap(10.)
        .center_y()
        .child(crate::app_chrome::brand_mark(26.))
        .child(
            col()
                .gap(2.)
                .grow(1.)
                .min_w(0.)
                .child(
                    text(releasescout_engine::APP_NAME)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base),
                )
                .child(line(status, k)),
        )
        .child(
            crate::sidebar_actions::action(
                "chronver-release-page",
                &glyphs::ARROW_RIGHT,
                "Open Chronver releases on GitHub",
                k,
            )
            .on_press_tx(|s: &mut State, tx| s.open_watched_notes(&Target::chronver(), tx)),
        )
        .child(
            Button::new(if following {
                "Unwatch Chronver"
            } else {
                "Watch Chronver"
            })
            .variant(ButtonVariant::Outline)
            .disabled(busy)
            .on_press(move |s: &mut State| {
                s.watch_request(Action::Toggle(Follow {
                    target: target.clone(),
                    title: releasescout_engine::APP_NAME.into(),
                }))
            })
            .build(k)
            .key("watch-chronver"),
        )
}

fn content(s: &State, k: &Kit, workspace: bool, feed_columns: usize) -> El {
    let state = &s.background_watch;
    let busy = state.work.is_pending();
    let mut body = col().w_full().gap(10.).px(16.).py(12.);
    if let Some((cfg, results)) = &state.snapshot {
        let enabled = cfg.enabled;
        let hr = if let Some(last) = results.last_check {
            format!(
                "{} verified / {} unverified  |  {}",
                results.verified,
                results.unverified,
                last.with_timezone(&chrono::Local).format("%b %d, %H:%M")
            )
        } else {
            "Not checked yet".into()
        };
        body = body.child(
            row()
                .gap(8.)
                .center_y()
                .child(
                    SegmentedControl::new(&["Off", "Hourly"], Some(usize::from(enabled)))
                        .width(Some(140.))
                        .label("Background check schedule")
                        .segment_state(
                            0,
                            if busy || !enabled {
                                rune_kit::State::Disabled
                            } else {
                                rune_kit::State::Rest
                            },
                        )
                        .segment_state(
                            1,
                            if busy || enabled || !cfg!(windows) {
                                rune_kit::State::Disabled
                            } else {
                                rune_kit::State::Rest
                            },
                        )
                        .on_pick(|s: &mut State, i, _| s.watch_request(Action::SetEnabled(i == 1)))
                        .build(k)
                        .key("watch-schedule"),
                )
                .child(
                    Button::new("Check now")
                        .disabled(busy)
                        .on_press(|s: &mut State| s.watch_request(Action::Check))
                        .build(k)
                        .key("watch-check-now"),
                )
                .child(line(if busy { "Working...".into() } else { hr }, k).grow(1.)),
        );
        if let Some(error) = &results.delivery_error {
            body = body.child(line(format!("Windows notification: {error}"), k));
        }
        body = body.child(
            SegmentedControl::new(&["Feed", "Watched", "Updates"], Some(state.tab))
                .width(Some(252.))
                .label("Background Watch view")
                .on_pick(|s: &mut State, i, _| {
                    s.background_watch.tab = i;
                    s.poll_watch_feed();
                })
                .build(k)
                .key("background-watch-tabs"),
        );
        let mut entries = col().w_full().gap(6.);
        if state.tab == FEED {
            entries = feed_entries(s, k, feed_columns);
        } else if state.tab == WATCHED {
            entries = entries.child(chronver_watch_row(cfg, results, busy, k));
            if !s.inventory.open
                && let Some(item) = s.selected().and_then(|p| follow(s, p))
                && !matches!(item.target, Target::Npm { .. })
                && !item.target.is_chronver()
            {
                let saved = cfg.items.iter().any(|i| i.target == item.target);
                entries = entries.child(
                    row()
                        .gap(8.)
                        .center_y()
                        .child(line(item.title.clone(), k).grow(1.))
                        .child(
                            Button::new(if saved { "Unwatch" } else { "Watch" })
                                .variant(ButtonVariant::Outline)
                                .disabled(busy)
                                .on_press(move |s: &mut State| {
                                    s.watch_request(Action::Toggle(item.clone()))
                                })
                                .build(k)
                                .key("watch-selected"),
                        ),
                );
            }
            for item in cfg.items.iter().filter(|item| !item.target.is_chronver()) {
                let key = item.target.key();
                let target = item.target.clone();
                entries = entries.child(
                    row()
                        .gap(8.)
                        .center_y()
                        .key(format!("watch-{key}"))
                        .child(
                            Button::new(&item.title)
                                .on_press_tx(move |s: &mut State, tx| {
                                    s.open_watched_notes(&target, tx)
                                })
                                .build(k)
                                .grow(1.)
                                .min_w(0.),
                        )
                        .child(
                            crate::sidebar_actions::action(
                                format!("unwatch-{key}"),
                                &glyphs::CLOSE,
                                "Unwatch",
                                k,
                            )
                            .disabled(busy)
                            .on_press(move |s: &mut State| {
                                s.watch_request(Action::Remove(key.clone()))
                            }),
                        ),
                );
            }
            let mut npm = s
                .packages
                .watch
                .entries
                .iter()
                .filter(|entry| entry.ecosystem == Ecosystem::Npm)
                .peekable();
            if npm.peek().is_some() {
                entries = entries.child(
                    row()
                        .gap(8.)
                        .px(8.)
                        .child(line("npm packages", k).grow(1.))
                        .child(line("Last verified", k)),
                );
                for entry in npm {
                    entries = entries.child(npm_row(s, k, entry, results));
                }
            } else if cfg.items.is_empty() {
                entries = entries.child(line("No watched items", k));
            }
        } else {
            if results.recent.is_empty() {
                entries = entries.child(line("No new updates detected", k));
            }
            for event in &results.recent {
                let target = event.item.target.clone();
                let key = format!("{}-{}", target.key(), event.version);
                entries = entries.child(
                    row()
                        .gap(8.)
                        .center_y()
                        .child(
                            Button::new(&event.item.title)
                                .on_press_tx(move |s: &mut State, tx| {
                                    s.open_watched_notes(&target, tx)
                                })
                                .build(k)
                                .key(key)
                                .min_w(0.)
                                .grow(1.),
                        )
                        .child(line(event.label(), k)),
                );
            }
        }
        let viewport = col()
            .w_full()
            .grow(1.)
            .min_h(0.)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(entries);
        let list = scroll_bar(
            k,
            "background-watch-list",
            "background-watch-scroll",
            viewport,
        )
        .min_h(0.);
        body = body.child(if workspace {
            list.grow(1.)
        } else {
            list.h(260.)
        });
    } else if busy {
        body = body.child(line("Loading watches", k));
    }
    if let Some(error) = &state.error {
        body = body.child(line(error, k).color(k.c.danger.fg));
    }
    body
}

pub fn workspace(s: &State, k: &Kit, cx: &mut rune::Cx<State>) -> El {
    const CARD_WIDTH: f32 = 348.;
    const GAP: f32 = 12.;
    let feed = s.background_watch.tab == FEED;
    let columns = if feed {
        cx.layout_with("watch-feed-grid", |rect| {
            ((rect[2] + GAP) / (CARD_WIDTH + GAP)).floor().max(1.) as usize
        })
        .unwrap_or(1)
    } else {
        1
    };
    let content = content(s, k, true, columns)
        .h_full()
        .min_h(0.)
        .pb(crate::app_chrome::DOCK_CLEARANCE);
    col()
        .key("watch-page")
        .w_full()
        .h_full()
        .min_h(0.)
        .fill(k.c.bg_base)
        .child(if feed { content } else { content.max_w(760.) })
}

pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>) {
    if !s.background_watch.open {
        return;
    }
    Dialog::new("background-watch", "Background Watch")
        .open(true)
        .fit()
        .width(570.)
        .on_close(|s: &mut State| s.background_watch.open = false)
        .child(content(s, k, false, 1))
        .build(k, layers);
}
