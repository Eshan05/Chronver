use crate::{
    live::cache_directory,
    model::{EntryKind, LibrarySection, Package, State},
};
use releasescout_engine::{
    Channel,
    http::{CacheOptions, HttpClient},
    watchlist::{self, Ecosystem, Entry, Group},
};
use rune::{El, Latest, TextEdit, col, row, text};
use rune_kit::{Button, IconButton, Kit, SegmentedControl, TextInput, glyphs};

pub struct Watch {
    pub entries: Vec<Entry>,
    pub loaded: bool,
    pub project_mode: bool,
    pub filter: usize,
    pub name: TextEdit,
    pub blocked: bool,
    pub dirty: bool,
    pub save_failed: bool,
    pub save: Latest<Result<(), String>>,
}
impl Default for Watch {
    fn default() -> Self {
        Self {
            entries: vec![],
            loaded: false,
            project_mode: false,
            filter: 0,
            name: TextEdit::new(""),
            blocked: false,
            dirty: false,
            save_failed: false,
            save: Latest::default(),
        }
    }
}
pub fn path() -> std::path::PathBuf {
    cache_directory().with_file_name("watchlist.json")
}
impl State {
    pub fn restore_watchlist(&mut self) {
        match watchlist::load(&path()) {
            Ok(entries) => {
                self.packages.watch.blocked = false;
                self.packages.watch.entries = entries;
                self.sync_watchlist();
            }
            Err(error) => {
                self.packages.watch.blocked = true;
                self.packages.error = Some(format!(
                    "Saved watchlist could not be loaded; file left unchanged: {error}"
                ));
            }
        }
        self.packages.watch.loaded = true;
    }
    pub fn sync_watchlist(&mut self) {
        self.packages.watch.loaded = true;
        let selected = self.selected().map(|p| p.id.clone());
        let entries = self.packages.watch.entries.clone();
        let alive: std::collections::HashSet<_> = entries.iter().map(Entry::id).collect();
        if self
            .live
            .active
            .as_ref()
            .is_some_and(|id| id.starts_with("watch-") && !alive.contains(id))
        {
            self.live.work.cancel();
            self.live.history.cancel();
            self.live.active = None;
        }
        self.catalog
            .packages
            .retain(|p| p.watchlist.is_none() || alive.contains(&p.id));
        self.live
            .catalog
            .packages
            .retain(|p| !p.id.starts_with("watch-") || alive.contains(&p.id));
        self.live
            .sessions
            .retain(|id, _| !id.starts_with("watch-") || alive.contains(id));
        for entry in entries {
            let id = entry.id();
            if let Some(p) = self.catalog.packages.iter_mut().find(|p| p.id == id) {
                p.manager = Some(format!("{} / {}", entry.ecosystem.id(), entry.group.id()));
                p.watchlist = Some(entry);
                continue;
            }
            let mut p:Package=serde_json::from_value(serde_json::json!({"id":id,"name":entry.name,"manager":format!("{} / {}",entry.ecosystem.id(),entry.group.id()),"repository":entry.url()})).unwrap();
            p.kind = EntryKind::Dependency;
            p.inventory_note = Some("Public releases".into());
            p.watchlist = Some(entry.clone());
            p.icon_plan = crate::package_icons::initial_plan(&p);
            self.catalog.packages.push(p);
            self.live.sessions.entry(id.clone()).or_default();
            let source_id = if entry.ecosystem == Ecosystem::Npm {
                entry.name.clone()
            } else {
                format!("cargo:{}", entry.name)
            };
            let mut profile = releasescout_engine::dependencies::known_package(&source_id)
                .ok()
                .flatten()
                .unwrap_or(releasescout_engine::Package {
                    id: id.clone(),
                    name: entry.name,
                    tracking: "track".into(),
                    identity: "personal_watchlist".into(),
                    alias_of: None,
                    caveats: String::new(),
                    reference_urls: vec![],
                    streams: vec![],
                });
            profile.id = id;
            self.live.catalog.packages.push(profile);
        }
        self.set_dataset(self.dataset);
        if let Some(id) = selected {
            self.select_package(&id);
        }
        if self.section == LibrarySection::Packages {
            self.status = self.inventory_summary();
        }
    }
    pub fn add_watchlist_entry(&mut self) {
        if self.packages.watch.blocked {
            return;
        }
        let entry = Entry {
            ecosystem: Ecosystem::Npm,
            name: self.packages.watch.name.text.trim().into(),
            group: Group::Core,
        };
        if let Err(error) = entry.validate() {
            self.packages.error = Some(error.to_string());
            return;
        }
        let id = entry.id();
        if !self.packages.watch.entries.iter().any(|e| e.id() == id) {
            if self.packages.watch.entries.len() >= 1000 {
                self.packages.error = Some("Watchlist limit reached".into());
                return;
            }
            self.packages.watch.entries.push(entry);
            self.packages.watch.dirty = true;
        }
        self.packages.error = None;
        self.packages.watch.name = TextEdit::new("");
        self.packages.watch.filter = 0;
        self.query = TextEdit::new("");
        self.sync_watchlist();
        self.select_package(&id);
    }
    pub fn remove_watchlist_entry(&mut self) {
        if self.packages.watch.blocked {
            return;
        }
        let Some(id) = self
            .selected()
            .filter(|p| p.watchlist.is_some())
            .map(|p| p.id.clone())
        else {
            return;
        };
        self.packages.watch.entries.retain(|e| e.id() != id);
        self.packages.watch.dirty = true;
        self.sync_watchlist();
    }
    pub fn set_watchlist_group(&mut self, index: usize) {
        if self.packages.watch.blocked {
            return;
        }
        let Some(id) = self
            .selected()
            .filter(|p| p.watchlist.is_some())
            .map(|p| p.id.clone())
        else {
            return;
        };
        if let Some(entry) = self
            .packages
            .watch
            .entries
            .iter_mut()
            .find(|e| e.id() == id)
        {
            entry.group = [Group::Core, Group::Tooling][index];
            self.packages.watch.dirty = true;
            self.sync_watchlist();
        }
    }
    pub fn poll_watchlist(&mut self) -> bool {
        let mut changed = false;
        if let Some(result) = self.packages.watch.save.take() {
            changed = true;
            self.packages.watch.save_failed = result.is_err();
            if result.is_ok()
                && self
                    .packages
                    .error
                    .as_ref()
                    .is_some_and(|e| e.starts_with("Watchlist not saved:"))
            {
                self.packages.error = None;
            }
            if let Err(error) = result {
                self.packages.error = Some(format!("Watchlist not saved: {error}"));
            }
        }
        if self.packages.persist
            && self.packages.watch.dirty
            && !self.packages.watch.save.is_pending()
        {
            let entries = self.packages.watch.entries.clone();
            self.packages.watch.dirty = false;
            self.packages.watch.save.request(entries, |entries| {
                watchlist::save(&path(), &entries).map_err(|e| e.to_string())
            });
            changed = true;
        }
        changed
    }
    pub fn fetch_watchlist_notes(&mut self) {
        if let Some(id) = self.selected().map(|p| p.id.clone()) {
            self.capture_note_feed(&id, false);
        }
        let Some(entry) = self.selected().and_then(|p| p.watchlist.clone()) else {
            return;
        };
        let id = entry.id();
        let q = &self.live.sessions[&id];
        let options = CacheOptions {
            directory: cache_directory(),
            public_only: true,
            offline: q.cache == 2,
            refresh: q.cache == 1,
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            ..Default::default()
        };
        let channel = [Channel::Stable, Channel::Preview, Channel::All][q.channel];
        self.live.history.cancel();
        self.live.active = Some(id.clone());
        let q = self.live.sessions.get_mut(&id).unwrap();
        q.error = None;
        q.report = None;
        q.next = None;
        q.shown = 20;
        q.caption = "Public package release history".into();
        self.catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .releases
            .clear();
        self.live.work.request(entry, move |entry| {
            let result = HttpClient::new(options)
                .and_then(|fetch| watchlist::notes(fetch, &entry, channel))
                .map_err(|e| e.to_string());
            (id, result)
        });
    }
}

pub fn bar(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    let watch = &s.packages.watch;
    let track = row().grow(1.).min_w(180.).center_y().gap(6.).px(6.)
        .radius(6.).border(1.,k.c.border_base).fill(k.c.bg_base)
        .child(crate::provider_badge::symbol_sized(releasescout_engine::inventory::Manager::Npm,k,14.).help("NPM package"))
        .child(TextInput::new(&watch.name).plain().height(28.).label("Package name").placeholder("Track a package...")
            .on_edit(|s: &mut State| &mut s.packages.watch.name)
            .on_submit(|s: &mut State,_|s.add_watchlist_entry()).build(k).grow(1.).min_w(0.).key("watchlist-name"))
        .child(IconButton::new(&glyphs::STAR).variant(rune_kit::IconButtonVariant::Ghost)
            .label("Track package").help("Track package").disabled(watch.blocked || watch.name.text.trim().is_empty())
            .on_press(|s: &mut State|s.add_watchlist_entry()).build(k).size(24.,24.).key("track-package"));
    let mut area = col()
        .w_full()
        .gap(8.0)
        .px(24.0)
        .py(12.0)
        .key("watchlist-bar")
        .child(
            row()
                .w_full()
                .center_y()
                .gap(8.0)
                .flex_wrap()
                .child(
                    SegmentedControl::new(
                        &["Watchlist", "Project scan"],
                        Some(usize::from(watch.project_mode)),
                    )
                    .width(Some(196.0))
                    .label("Packages view")
                    .on_pick(|s: &mut State, i, tx| {
                        s.packages.watch.project_mode = i == 1;
                        s.query = TextEdit::new("");
                        s.observation_picker = false;
                        s.set_dataset(s.dataset);
                        s.status = s.inventory_summary();
                        tx.instant();
                    })
                    .build(k)
                    .key("packages-view"),
                )
                .children((!watch.project_mode).then(|| {
                    SegmentedControl::new(&["All", "Core", "Tooling"], Some(watch.filter))
                        .width(Some(220.0))
                        .label("Watchlist category")
                        .on_pick(|s: &mut State, i, tx| {
                            s.packages.watch.filter = i;
                            s.set_dataset(s.dataset);
                            s.status = s.inventory_summary();
                            tx.instant();
                        })
                        .build(k)
                        .key("watchlist-filter")
                }))
                .children((!watch.project_mode).then_some(track)),
        );
    if watch.project_mode {
        return col()
            .w_full()
            .child(area)
            .child(crate::package_ui::project_bar(s, k, layers));
    }
    if let Some(error) = &s.packages.error {
        area = area.child(
            text(error)
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted)
                .wrap_anywhere()
                .key("watchlist-error"),
        );
    }
    if watch.save_failed {
        area = area.child(
            Button::new("Retry save")
                .on_press(|s: &mut State| s.packages.watch.dirty = true)
                .build(k)
                .key("retry-watchlist-save"),
        );
    }
    if watch.blocked {
        area = area.child(
            Button::new("Reload watchlist")
                .on_press(|s: &mut State| {
                    s.packages.error = None;
                    s.restore_watchlist();
                })
                .build(k)
                .key("reload-watchlist"),
        );
    }
    area.border_sides([0.0, 0.0, 1.0, 0.0], crate::theme::separator(k))
}

pub fn category_badge(group: Group, k: &Kit) -> El {
    let (label, id, dark, light) = match group {
        Group::Core => ("Core", "core", "#92bda6", "#46795b"),
        Group::Tooling => ("Tooling", "tooling", "#98b5d0", "#4d7596"),
    };
    let tint = rune::color::hex(if k.dark { dark } else { light }).unwrap();
    row()
        .h(18.)
        .px(4.)
        .center_y()
        .radius(4.)
        .fill(tint.alpha(if k.dark { 0.12 } else { 0.08 }))
        .border(0.5, tint.alpha(0.3))
        .role("group")
        .label(label)
        .key(format!("category-badge-{id}"))
        .child(
            text(label)
                .font(k.ty.small_medium)
                .color(k.c.text_muted.mix(tint, 0.65))
                .a11y_hidden(),
        )
}

pub fn controls(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    crate::package_ui::controls(s,k,layers)
}

pub fn scene(s: &mut State) {
    s.packages.watch.entries = vec![
        Entry {
            ecosystem: Ecosystem::Npm,
            name: "next".into(),
            group: Group::Core,
        },
        Entry {
            ecosystem: Ecosystem::Npm,
            name: "zod".into(),
            group: Group::Core,
        },
        Entry {
            ecosystem: Ecosystem::Npm,
            name: "typescript".into(),
            group: Group::Tooling,
        },
        Entry {
            ecosystem: Ecosystem::Cargo,
            name: "serde".into(),
            group: Group::Core,
        },
    ];
    s.sync_watchlist();
    s.set_section(LibrarySection::Packages);
}

pub fn audited_scene(s: &mut State) {
    s.packages.watch.entries = watchlist::load(&path()).expect("explicit saved-watchlist render");
    s.sync_watchlist();
    s.set_section(LibrarySection::Packages);
    if let Some(entry) = s
        .packages
        .watch
        .entries
        .iter()
        .find(|e| e.name == "next")
        .cloned()
    {
        s.select_package(&entry.id());
    }
}

pub fn notes_scene(s: &mut State) {
    audited_scene(s);
    let entry = s.selected().unwrap().watchlist.clone().unwrap();
    s.engine_query().cache = 2;
    s.engine_query().caption = "Public package release history".into();
    let result = HttpClient::new(CacheOptions {
        directory: cache_directory(),
        public_only: true,
        offline: true,
        github_token: std::env::var("GITHUB_TOKEN").ok(),
        ..Default::default()
    })
    .and_then(|fetch| watchlist::notes(fetch, &entry, Channel::Stable))
    .map_err(|e| e.to_string());
    s.apply_engine_result(entry.id(), result);
}
