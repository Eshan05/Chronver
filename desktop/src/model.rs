use rune::TextEdit;
use rune_kit::{SortDir, TableState, shell::PaneState};
use serde::Deserialize;

#[derive(Default, Deserialize)]
pub struct Catalog {
    pub captured: String,
    #[serde(default)]
    pub source: String,
    pub packages: Vec<Package>,
}

#[derive(Clone, Default, Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(skip)]
    pub description_fallback: Option<&'static crate::descriptions::Entry>,
    pub manager: Option<String>,
    #[serde(skip)]
    pub provider: Option<releasescout_engine::inventory::Manager>,
    #[serde(skip)]
    pub observation: Option<crate::sidebar_info::Observation>,
    pub repository: Option<String>,
    pub installed: Option<String>,
    pub target: Option<String>,
    pub reference_text: Option<String>,
    #[serde(default)]
    pub kind: EntryKind,
    #[serde(default)]
    pub removed: bool,
    #[serde(default)]
    pub unconfirmed: bool,
    #[serde(skip)]
    pub sample_notes: bool,
    #[serde(default)]
    pub releases: Vec<Release>,
    #[serde(skip)]
    pub detected: bool,
    #[serde(skip)]
    pub reference_id: Option<String>,
    #[serde(skip)]
    pub inventory_note: Option<String>,
    #[serde(skip)]
    pub group_key: Option<String>,
    #[serde(skip)]
    pub component_reason: Option<String>,
    #[serde(skip)]
    pub observation_label: String,
    #[serde(skip)]
    pub display_priority: u8,
    #[serde(skip)]
    pub icon: Option<rune::Image>,
    #[serde(skip)]
    pub custom_icon: Option<rune::Image>,
    #[serde(skip)]
    pub custom_symbol: Option<&'static rune::Icon>,
    #[serde(default, rename = "icon_sources")]
    pub icon_plan: Option<releasescout_engine::icons::Plan>,
    #[serde(skip)]
    pub icon_source: Option<String>,
    #[serde(skip)]
    pub extension: Option<releasescout_engine::extensions::Installed>,
    #[serde(skip)]
    pub dependency: Option<releasescout_engine::dependencies::Dependency>,
    #[serde(skip)]
    pub watchlist: Option<releasescout_engine::watchlist::Entry>,
    #[serde(skip)]
    pub source_hints: Option<releasescout_engine::source_discovery::Hints>,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    #[default]
    Package,
    App,
    Extension,
    Dependency,
}

impl Package {
    pub fn manager_label(&self) -> &str {
        self.manager.as_deref().unwrap_or("Method unrecorded")
    }

    pub fn installation_label(&self) -> &str {
        if self.watchlist.is_some() {
            "Tracked package"
        } else if self.dependency.is_some() {
            "Project dependency"
        } else if self.detected {
            "Detected in last scan"
        } else if self.removed {
            "No longer installed"
        } else if self.unconfirmed {
            "Installation unconfirmed"
        } else {
            "Demo entry"
        }
    }

    pub fn reference_file(&self) -> &str {
        if self.dependency.is_some() {
            "package.json"
        } else if self.detected {
            "Local inventory"
        } else {
            "Demo data"
        }
    }

    pub fn subtitle(&self) -> String {
        if self.removed || self.unconfirmed {
            self.installation_label().into()
        } else if self.kind == EntryKind::App {
            format!("App / {}", self.manager_label())
        } else {
            self.manager_label().into()
        }
    }

    pub fn update_label(&self) -> &str {
        if let Some(note) = &self.inventory_note {
            note
        } else if self.removed {
            "Not tracked / no longer installed"
        } else {
            "Updates not checked"
        }
    }

    pub fn monogram(&self) -> String {
        let label: String = self
            .name
            .split(|c: char| !c.is_alphanumeric())
            .find(|s| !s.is_empty())
            .unwrap_or("")
            .chars()
            .flat_map(char::to_uppercase)
            .take(2)
            .collect();
        if label.is_empty() { ">_".into() } else { label }
    }
}

#[derive(Clone, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub published: String,
    pub markdown: String,
}

pub struct PackageRow {
    pub id: String,
    pub name: String,
    pub package: usize,
    pub members: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Dataset {
    Detected,
    Reference,
    Stress,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub enum LibrarySection {
    #[default]
    Software,
    Extensions,
    Packages,
}

#[derive(Default)]
struct SectionState {
    query: String,
    selection: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum NoteMode {
    Rendered,
    Source,
    Ai,
}

pub struct State {
    pub backdrop: crate::native_backdrop::Backdrop,
    pub reader_controls: crate::reader_controls::Controls,
    pub chrome: crate::app_chrome::Chrome,
    pub command: crate::command::Palette,
    pub ai: crate::ai_menu::Ui,
    pub sidebar_groups: crate::sidebar_groups::Groups,
    pub sidebar_actions: crate::sidebar_actions::Actions,
    pub description_editor: Option<crate::description_ui::Editor>,
    pub description_hover: Option<crate::description_preview::Hover>,
    pub sidebar_sort_open: bool,
    pub sidebar_filters_open: bool,
    pub custom_icons: crate::custom_icons::CustomIcons,
    pub activity: crate::activity::Activity,
    pub source_loader: crate::source_loader::Loader,
    pub note_cache: crate::note_cache::Cache,
    pub note_prefetch: crate::note_prefetch::Prefetch,
    pub icon_loader: crate::icon_loader::Loader,
    pub icons: rune::ImageCache,
    pub icons_ready: usize,
    pub section: LibrarySection,
    section_states: [SectionState; 3],
    pub packages: crate::package_ui::Library,
    pub package_icons: crate::package_icons::Loader,
    pub extensions: crate::extension_ui::Filter,
    pub show_components: bool,
    pub observation_picker: bool,
    pub inventory: crate::inventory_ui::Inventory,
    pub live: crate::live::Live,
    pub catalog: Catalog,
    pub rows: Vec<PackageRow>,
    pub visible: Vec<usize>,
    pub table: TableState,
    pub query: TextEdit,
    pub pane: PaneState,
    pub dataset: Dataset,
    pub mode: NoteMode,
    pub dark: bool,
    pub status: String,
    pub sidebar_scroll: crate::sidebar::WheelScroll,
    pub list_revision: u64,
}

impl State {
    pub fn display_name<'a>(&'a self, package: &'a Package) -> &'a str {
        let names = &self.activity.data.name_overrides;
        if names.is_empty() {
            return &package.name;
        }
        names
            .get(&crate::custom_icons::identity(package))
            .map_or(package.name.as_str(), String::as_str)
    }

    pub fn new() -> Self {
        let mut state = Self {
            backdrop: Default::default(),
            reader_controls: Default::default(),
            chrome: Default::default(),
            command: Default::default(),
            ai: Default::default(),
            sidebar_groups: Default::default(),
            sidebar_actions: Default::default(),
            description_editor: None,
            description_hover: None,
            sidebar_sort_open: false,
            sidebar_filters_open: false,
            custom_icons: Default::default(),
            activity: Default::default(),
            source_loader: Default::default(),
            note_cache: Default::default(),
            note_prefetch: Default::default(),
            icon_loader: Default::default(),
            icons: rune::ImageCache::new(std::sync::Arc::new(|| {})),
            icons_ready: 0,
            section: LibrarySection::Software,
            section_states: Default::default(),
            packages: Default::default(),
            package_icons: Default::default(),
            extensions: Default::default(),
            show_components: false,
            observation_picker: false,
            inventory: crate::inventory_ui::Inventory::default(),
            live: crate::live::Live::new(),
            catalog: Catalog::default(),
            rows: Vec::new(),
            visible: Vec::new(),
            table: TableState::new().key("packages"),
            query: TextEdit::new(""),
            pane: PaneState::new(304.0).range(304.0, 420.0),
            dataset: Dataset::Detected,
            mode: NoteMode::Rendered,
            dark: true,
            status: "Inventory not scanned".into(),
            sidebar_scroll: crate::sidebar::WheelScroll::default(),
            list_revision: 0,
        };
        state.table.set_sort("name", SortDir::Ascending);
        state
    }

    pub fn demo() -> Self {
        let mut state = Self::new();
        let mut samples: Catalog = serde_json::from_str(include_str!("../fixtures/demo-releases.json"))
            .expect("release samples must be valid");
        state.catalog.captured = samples.captured;
        state.catalog.source = "Engine catalog and synthetic release notes (demo only)".into();
        state.status = "Demo data / no scan".into();
        state.catalog.packages = state
            .live
            .catalog
            .packages
            .iter()
            .map(|profile| {
                let sample = samples
                    .packages
                    .iter()
                    .position(|p| p.id == profile.id)
                    .map(|i| samples.packages.swap_remove(i));
                Package {
                    id: profile.id.clone(),
                    name: if profile.id == "agent-browser" {
                        "Agent browser".into()
                    } else {
                        crate::display_names::software(&profile.name)
                    },
                    description_fallback: crate::descriptions::reference(&profile.id),
                    icon_plan: profile
                        .reference_urls
                        .iter()
                        .find_map(|u| releasescout_engine::icons::product_homepage(u))
                        .map(|homepage| releasescout_engine::icons::Plan {
                            homepage: Some(homepage),
                            icon_url: None,
                        }),
                    sample_notes: sample.is_some(),
                    repository: sample.as_ref().and_then(|p| p.repository.clone()),
                    releases: sample.map(|p| p.releases).unwrap_or_default(),
                    ..Package::default()
                }
            })
            .collect();
        state.set_dataset(Dataset::Reference);
        state
    }

    pub fn set_dataset(&mut self, dataset: Dataset) {
        if self.dataset != dataset
            || self.command.revealed.as_ref().is_some_and(|id| {
                !self.catalog.packages.iter().any(|p| &p.id == id)
            })
        {
            self.command.revealed = None;
        }
        if dataset != Dataset::Detected && self.section == LibrarySection::Extensions {
            self.remember_section();
            self.section = LibrarySection::Software;
            self.query = TextEdit::new(&self.section_states[0].query);
        }
        self.sidebar_scroll.reset();
        self.dataset = dataset;
        let indices: Vec<_> = self
            .catalog
            .packages
            .iter()
            .enumerate()
            .filter(|(_, p)| crate::command::reveals(self, p) || match self.section {
                LibrarySection::Packages => {
                    if self.packages.watch.project_mode {
                        p.dependency.as_ref().is_some_and(|d| {
                            self.packages.project.is_empty() || d.project == self.packages.project
                        })
                    } else {
                        p.watchlist.as_ref().is_some_and(|entry| {
                            entry.ecosystem == releasescout_engine::watchlist::Ecosystem::Npm
                                && match self.packages.watch.filter {
                                    1 => entry.group == releasescout_engine::watchlist::Group::Core,
                                    2 => {
                                        entry.group
                                            == releasescout_engine::watchlist::Group::Tooling
                                    }
                                    _ => true,
                                }
                        })
                    }
                }
                LibrarySection::Extensions => {
                    p.detected && p.kind == EntryKind::Extension && self.extensions.matches(p)
                }
                LibrarySection::Software => {
                    p.dependency.is_none()
                        && p.watchlist.is_none()
                        && p.kind != EntryKind::Extension
                        && p.detected == (dataset == Dataset::Detected)
                }
            })
            .map(|(i, _)| i)
            .collect();
        let stress = dataset == Dataset::Stress
            && self.section == LibrarySection::Software
            && !indices.is_empty();
        let count = if stress { 10_000 } else { indices.len() };
        self.rows = (0..count)
            .map(|i| {
                let package = indices[i % indices.len()];
                let p = &self.catalog.packages[package];
                PackageRow {
                    id: if stress {
                        format!("{}-{i}", p.id)
                    } else {
                        p.id.clone()
                    },
                    name: if stress {
                        format!("{} #{i:05}", self.display_name(p))
                    } else {
                        self.display_name(p).into()
                    },
                    package,
                    members: vec![package],
                }
            })
            .collect();
        if dataset == Dataset::Detected || self.section == LibrarySection::Packages {
            let mut grouped: Vec<PackageRow> = vec![];
            let mut positions = std::collections::HashMap::new();
            for mut row in self.rows.drain(..) {
                let p = &self.catalog.packages[row.package];
                if self
                    .live
                    .catalog
                    .packages
                    .iter()
                    .any(|profile| profile.id == p.id && profile.tracking == "excluded")
                {
                    continue;
                }
                if !self.show_components && p.component_reason.is_some() {
                    continue;
                }
                let mut key = p.group_key.as_ref().unwrap_or(&p.id).clone();
                if self.section == LibrarySection::Extensions {
                    key = format!(
                        "extension-row-{}",
                        crate::activity::hash((&key, crate::extension_ui::profile_key(p)))
                    );
                }
                if let Some(&i) = positions.get(&key) {
                    let group: &mut PackageRow = &mut grouped[i];
                    group.members.push(row.package);
                    if p.display_priority < self.catalog.packages[group.package].display_priority {
                        group.package = row.package;
                        group.name = row.name;
                    }
                } else {
                    positions.insert(key.clone(), grouped.len());
                    row.id = key;
                    grouped.push(row);
                }
            }
            self.rows = grouped;
        }
        self.table.clear_selection();
        self.refilter();
    }

    fn remember_section(&mut self) {
        self.section_states[self.section as usize] = SectionState {
            query: self.query.text.clone(),
            selection: self.selected().map(|p| p.id.clone()),
        };
    }

    pub fn set_section(&mut self, section: LibrarySection) {
        self.reader_controls.inventory_open = None;
        self.reader_controls.reset_scroll();
        self.reader_controls.menu = None;
        self.reader_controls.release_index_open = false;
        if (self.dataset != Dataset::Detected && section == LibrarySection::Extensions)
            || self.section == section
        {
            return;
        }
        self.command.revealed = None;
        self.remember_section();
        if section == LibrarySection::Packages && !self.packages.watch.loaded {
            self.sync_watchlist();
        }
        if let Some(id) = self.selected().map(|p| p.id.clone())
            && let Some(query) = self.live.sessions.get_mut(&id)
        {
            query.component_open = false;
        }
        self.section = section;
        self.sidebar_filters_open = false;
        self.query = TextEdit::new(&self.section_states[section as usize].query);
        self.observation_picker = false;
        self.set_dataset(self.dataset);
        if let Some(id) = self.section_states[section as usize].selection.clone() {
            self.select_package(&id);
        }
        self.status = self.inventory_summary();
    }

    pub fn refilter(&mut self) {
        self.sidebar_scroll.reset();
        let query = self.query.text.trim().to_lowercase();
        self.visible = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.members.iter().any(|&i| crate::command::reveals(self, &self.catalog.packages[i]))
                    || crate::sidebar_filters::matches(self, r)
            })
            .filter(|(_, r)| {
                let p = &self.catalog.packages[r.package];
                r.members.iter().any(|&i| crate::command::reveals(self, &self.catalog.packages[i]))
                    || query.is_empty()
                    || r.members.iter().any(|&i| {
                        let p = &self.catalog.packages[i];
                        format!("{} {} {}", p.name, p.manager_label(), p.observation_label)
                            .to_lowercase()
                            .contains(&query)
                    })
                    || r.name.to_lowercase().contains(&query)
                    || p.manager_label().to_lowercase().contains(&query)
                    || p.reference_text
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        self.sort_visible();
        let survives = self
            .table
            .cursor()
            .is_some_and(|id| self.visible.iter().any(|&i| self.rows[i].id == id));
        if !survives {
            self.table.clear_selection();
            let first = crate::sidebar_groups::shown(self)
                .first()
                .copied()
                .or_else(|| self.visible.first().copied());
            if let Some(i) = first {
                self.table.select(self.rows[i].id.clone());
            }
        }
    }

    pub fn sort_visible(&mut self) {
        self.sidebar_scroll.reset();
        self.list_revision = self.list_revision.wrapping_add(1);
        crate::sidebar_sort::sort(self);
        self.rebuild_sidebar_groups();
    }

    pub fn selected(&self) -> Option<&Package> {
        let id = self.table.cursor()?;
        self.visible
            .iter()
            .map(|&i| &self.rows[i])
            .find(|r| r.id == id)
            .map(|r| &self.catalog.packages[r.package])
    }

    pub fn select_package(&mut self, id: &str) {
        self.reader_controls.inventory_open = None;
        self.reader_controls.reset_scroll();
        self.reader_controls.menu = None;
        self.reader_controls.release_index_open = false;
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|r| r.id == id || r.members.iter().any(|&i| self.catalog.packages[i].id == id))
        {
            if let Some(&i) = row
                .members
                .iter()
                .find(|&&i| self.catalog.packages[i].id == id)
            {
                row.package = i;
            }
            self.table.select(row.id.clone());
        }
    }
    pub fn toggle_components(&mut self) {
        self.command.revealed = None;
        let selected = self.selected().map(|p| p.id.clone());
        self.show_components = !self.show_components;
        self.activity.data.sidebar_filters[LibrarySection::Software as usize].include_support =
            self.show_components;
        self.activity.dirty = true;
        self.set_dataset(self.dataset);
        if let Some(id) = selected {
            self.select_package(&id);
        }
        self.status = self.inventory_summary();
    }
    pub fn selected_observations(&self) -> Vec<&Package> {
        self.rows
            .iter()
            .find(|r| Some(r.id.as_str()) == self.table.cursor())
            .map(|r| {
                r.members
                    .iter()
                    .map(|&i| &self.catalog.packages[i])
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn selected_markdown(&self) -> String {
        if let Some(p) = self.selected()
            && let Some(report) = self
                .live
                .sessions
                .get(&p.id)
                .and_then(|q| q.report.as_ref())
        {
            return format!(
                "{}\n{}\n\n{}\n\n{}",
                report.scope,
                self.live.sessions[&p.id].caption,
                report.warnings.join("\n"),
                report
                    .notes
                    .iter()
                    .map(|n| format!("# {}\n\nSource: {}\n\n{}", n.title, n.url, n.body))
                    .collect::<Vec<_>>()
                    .join("\n\n---\n\n")
            );
        }
        self.selected()
            .map(|p| {
                let body = p
                    .releases
                    .iter()
                    .map(|r| {
                        format!(
                            "# {} {}\n\nSource: {}\n\n{}\n",
                            p.name, r.version, r.url, r.markdown
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n---\n\n");
                if p.sample_notes && !body.is_empty() {
                    format!(
                        "Synthetic sample notes; not published releases.\n\n{body}"
                    )
                } else {
                    body
                }
            })
            .unwrap_or_default()
    }
}
