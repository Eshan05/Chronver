use crate::{
    live::cache_directory,
    model::{EntryKind, LibrarySection, Package, State},
    rich_select::{self, OptionRow, RichSelect},
};
use releasescout_engine::{
    Channel,
    dependencies::{self, Scan},
    http::{CacheOptions, HttpClient},
};
use rune::{El, Latest, TextEdit, col, icon, row, text};
use rune_kit::{IconButton, Kit, TextInput, glyphs};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub struct Library {
    pub watch: crate::watchlist_ui::Watch,
    pub path: TextEdit,
    pub project: String,
    pub picker: bool,
    pub scan: Option<Scan>,
    pub error: Option<String>,
    pub work: Latest<Result<Scan, String>>,
    pub persist: bool,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            watch: Default::default(),
            path: TextEdit::new(""),
            project: String::new(),
            picker: false,
            scan: None,
            error: None,
            work: Latest::default(),
            persist: false,
        }
    }
}
fn settings() -> PathBuf {
    cache_directory().with_file_name("project.json")
}

fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(unc) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        path.strip_prefix("\\\\?\\").unwrap_or(&path).into()
    }
}

impl State {
    pub fn restore_project(&mut self) {
        self.packages.persist = true;
        self.restore_watchlist();
        let mut bytes = Vec::new();
        if std::fs::File::open(settings())
            .and_then(|file| file.take(64 * 1024 + 1).read_to_end(&mut bytes))
            .is_ok()
            && bytes.len() <= 64 * 1024
            && let Ok(path) = serde_json::from_slice::<PathBuf>(&bytes)
        {
            self.packages.path = TextEdit::new(display_path(&path));
        }
    }
    pub fn scan_project(&mut self) {
        let path = PathBuf::from(self.packages.path.text.trim());
        if self.packages.path.text.trim().is_empty() {
            self.packages.error = Some("Choose a project folder".into());
            return;
        }
        self.packages.error = None;
        self.packages.work.request(path, |path| {
            dependencies::scan(&path).map_err(|e| e.to_string())
        });
    }
    pub fn poll_packages(&mut self) -> bool {
        let Some(result) = self.packages.work.take() else {
            return false;
        };
        match result {
            Ok(scan) => {
                if self.packages.persist {
                    let path = settings();
                    let save = (|| -> std::io::Result<()> {
                        std::fs::create_dir_all(path.parent().unwrap())?;
                        let tmp = path.with_extension("tmp");
                        std::fs::write(&tmp, serde_json::to_vec(&scan.root)?)?;
                        std::fs::rename(tmp, path)
                    })();
                    if let Err(error) = save {
                        self.packages.error =
                            Some(format!("Project preference could not be saved: {error}"));
                    }
                }
                self.accept_packages(scan);
            }
            Err(error) => self.packages.error = Some(format!("{error}; previous project retained")),
        }
        true
    }
    pub fn accept_packages(&mut self, scan: Scan) {
        let selected = self.selected().map(|p| p.id.clone());
        // A scan invalidates package-note results, even if an unchanged ID survives it.
        if self
            .live
            .active
            .as_ref()
            .is_some_and(|id| id.starts_with("dependency-"))
        {
            self.live.work.cancel();
            self.live.history.cancel();
            self.live.active = None;
        }
        self.catalog.packages.retain(|p| p.dependency.is_none());
        self.live
            .sessions
            .retain(|id, _| !id.starts_with("dependency-"));
        self.live
            .catalog
            .packages
            .retain(|p| !p.id.starts_with("dependency-"));
        for dependency in &scan.dependencies {
            let id = dependency.id.clone();
            let mut p: Package = serde_json::from_value(serde_json::json!({
                "id":id,"name":dependency.package,"manager":format!("npm / {}", dependency.kind),
                "installed":dependency.installed,"reference_text":format!("{} / {} / declared {}", dependency.project, dependency.alias, dependency.requirement)
            })).unwrap();
            p.kind = EntryKind::Dependency;
            p.repository = dependency.repository.clone();
            p.group_key = Some(format!("dependency-group:{}", dependency.package));
            p.observation_label = format!(
                "{} / {} / declared {} / installed {}",
                dependency.project,
                dependency.kind,
                dependency.requirement,
                dependency.installed.as_deref().unwrap_or("unresolved")
            );
            p.inventory_note = Some(format!(
                "Declared {} / installed {}",
                dependency.requirement,
                dependency.installed.as_deref().unwrap_or("unresolved")
            ));
            p.dependency = Some(dependency.clone());
            p.icon_plan = crate::package_icons::initial_plan(&p);
            self.live.sessions.insert(id.clone(), Default::default());
            let mut profile = dependency
                .public_registry
                .then(|| {
                    releasescout_engine::dependencies::known_package(&dependency.package)
                        .ok()
                        .flatten()
                })
                .flatten()
                .unwrap_or(releasescout_engine::Package {
                    id: id.clone(),
                    name: dependency.package.clone(),
                    tracking: "track".into(),
                    identity: "project_dependency".into(),
                    alias_of: None,
                    caveats: String::new(),
                    reference_urls: vec![],
                    streams: vec![],
                });
            profile.id = id;
            self.live.catalog.packages.push(profile);
            self.catalog.packages.push(p);
        }
        if !scan
            .dependencies
            .iter()
            .any(|d| d.project == self.packages.project)
        {
            self.packages.project.clear();
        }
        self.packages.path = TextEdit::new(display_path(&scan.root));
        self.packages.scan = Some(scan);
        self.set_dataset(self.dataset);
        if let Some(id) = selected {
            self.select_package(&id);
        }
        if self.section == LibrarySection::Packages {
            self.status = self.inventory_summary();
        }
    }
    pub fn fetch_dependency_notes(&mut self) {
        if let Some(id) = self.selected().map(|p| p.id.clone()) {
            self.capture_note_feed(&id, false);
        }
        let Some(dependency) = self.selected().and_then(|p| p.dependency.clone()) else {
            return;
        };
        let id = dependency.id.clone();
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
        q.caption = "Package source history / constraint compatibility not evaluated".into();
        self.catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .releases
            .clear();
        self.live.work.request(dependency, move |dependency| {
            let result = HttpClient::new(options)
                .and_then(|fetch| dependencies::notes(fetch, &dependency, channel))
                .map_err(|e| e.to_string());
            (id, result)
        });
    }
}

fn label(value: impl Into<String>, k: &Kit) -> El {
    text(value)
        .font(k.ty.text_xsmall)
        .color(k.c.text_muted)
        .wrap_anywhere()
}

pub fn project_bar(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    let mut bar = col()
        .w_full()
        .px(28.0)
        .py(14.0)
        .gap(10.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], crate::theme::separator(k))
        .key("project-controls")
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    TextInput::new(&s.packages.path)
                        .label("Project folder")
                        .placeholder("Project folder...")
                        .on_edit(|s: &mut State| &mut s.packages.path)
                        .on_submit(|s: &mut State, _| s.scan_project())
                        .build(k)
                        .grow(1.0)
                        .min_w(0.0)
                        .key("project-path"),
                )
                .child(
                    IconButton::new(&glyphs::FOLDER_OPEN)
                        .label("Open project folder")
                        .help("Open project folder")
                        .on_press_tx(|s: &mut State, tx| {
                            tx.open_folder(
                                rune::FileDialogOptions {
                                    title: "Open project".into(),
                                    directory: s.packages.scan.as_ref().map(|s| s.root.clone()),
                                    ..Default::default()
                                },
                                |s: &mut State, result, _| match result {
                                    Ok(Some(path)) => {
                                        s.packages.path = TextEdit::new(display_path(&path));
                                        s.scan_project();
                                    }
                                    Ok(None) => {}
                                    Err(error) => s.packages.error = Some(error.to_string()),
                                },
                            );
                        })
                        .build(k)
                        .key("open-project"),
                )
                .child(
                    IconButton::new(&glyphs::SEARCH)
                        .label("Scan project dependencies")
                        .help("Scan project dependencies")
                        .disabled(s.packages.work.is_pending())
                        .on_press(|s: &mut State| s.scan_project())
                        .build(k)
                        .key("scan-project"),
                )
                .children(s.packages.work.is_pending().then(|| {
                    IconButton::new(&glyphs::CLOSE)
                        .label("Cancel project scan")
                        .help("Cancel project scan")
                        .on_press(|s: &mut State| {
                            s.packages.work.cancel();
                            s.packages.error =
                                Some("Scan canceled; previous project retained".into());
                        })
                        .build(k)
                        .key("cancel-project-scan")
                })),
        );
    if let Some(scan) = &s.packages.scan {
        let mut projects = vec!["All projects".to_string()];
        let mut summaries = std::collections::BTreeMap::new();
        for d in &scan.dependencies {
            let summary = summaries
                .entry(d.project.as_str())
                .or_insert((0, d.manifest.as_path()));
            summary.0 += 1;
        }
        projects.extend(summaries.keys().map(|name| name.to_string()));
        bar = bar.child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(
                    RichSelect {
                        key: "project-filter",
                        label: "Project",
                        current: &s.packages.project,
                        open: s.packages.picker,
                        popup_width: 440.,
                        value: row()
                            .gap(8.)
                            .center_y()
                            .child(icon(&glyphs::FOLDER, 16.).color(k.c.text_muted))
                            .child(
                                text(if s.packages.project.is_empty() {
                                    "All projects"
                                } else {
                                    &s.packages.project
                                })
                                .font(k.ty.ui_compact)
                                .color(k.c.text_base)
                                .grow(1.)
                                .min_w(0.)
                                .truncate(),
                            ),
                        options: projects
                            .iter()
                            .enumerate()
                            .map(|(i, project)| {
                                let detail = if i == 0 {
                                    format!(
                                        "{} / {}",
                                        rich_select::count_label(scan.projects.len(), "manifest"),
                                        rich_select::count_label(
                                            scan.dependencies.len(),
                                            "declaration"
                                        )
                                    )
                                } else {
                                    let (count, manifest) = summaries[project.as_str()];
                                    format!(
                                        "{} / {}",
                                        rich_select::count_label(count, "declaration"),
                                        display_path(manifest)
                                    )
                                };
                                OptionRow {
                                    id: if i == 0 {
                                        String::new()
                                    } else {
                                        project.clone()
                                    },
                                    label: project.clone(),
                                    description: detail.clone(),
                                    content: row()
                                        .gap(10.)
                                        .center_y()
                                        .child(icon(&glyphs::FOLDER, 20.).color(k.c.text_muted))
                                        .child(
                                            rich_select::text_content(project, &detail, k).grow(1.),
                                        ),
                                }
                            })
                            .collect(),
                        on_toggle: |s: &mut State| s.packages.picker = !s.packages.picker,
                        on_close: |s: &mut State| s.packages.picker = false,
                        on_choose: |s: &mut State, value, tx| {
                            s.packages.project = value.into();
                            s.set_dataset(s.dataset);
                            s.status = s.inventory_summary();
                            tx.instant();
                        },
                    }
                    .build(k, layers)
                    .w(320.),
                )
                .child(label(
                    format!(
                        "{} manifests / {} declarations",
                        scan.projects.len(),
                        scan.dependencies.len()
                    ),
                    k,
                )),
        );
        if let Some(warning) = scan.warnings.first() {
            bar = bar.child(
                label(
                    format!(
                        "{}{}",
                        warning,
                        if scan.warnings.len() > 1 {
                            format!(" (+{} warnings)", scan.warnings.len() - 1)
                        } else {
                            String::new()
                        }
                    ),
                    k,
                )
                .help(scan.warnings.join("\n")),
            );
        }
    }
    if s.packages.work.is_pending() {
        bar = bar.child(label("Scanning project manifests...", k));
    }
    if let Some(error) = &s.packages.error {
        bar = bar.child(label(error, k).key("project-error"));
    }
    bar
}

pub fn controls(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    let p = s.selected().unwrap();
    crate::reader_controls::area("dependency-controls")
        .children(p.watchlist.is_none().then(||label(p.update_label(),k).key("dependency-versions")))
        .child(crate::reader_controls::actions(s,k,layers,true))
}

pub fn scene(s: &mut State) {
    s.packages.watch.project_mode = true;
    let root = PathBuf::from("C:/fixture/workspace");
    let dependency = |id: &str, project: &str, kind: &str, spec: &str, installed: Option<&str>| {
        dependencies::Dependency {
            id: format!("dependency-{id}"),
            ecosystem: dependencies::Ecosystem::Npm,
            project: project.into(),
            manifest: root.join(format!("{project}/package.json")),
            package: "next".into(),
            alias: "next".into(),
            requirement: spec.into(),
            kind: kind.into(),
            installed: installed.map(String::from),
            root: None,
            repository: None,
            directory: None,
            homepage: None,
            public_registry: true,
        }
    };
    s.accept_packages(Scan {
        root: root.clone(),
        projects: vec![
            dependencies::Project {
                name: "web".into(),
                manifest: root.join("web/package.json"),
            },
            dependencies::Project {
                name: "docs".into(),
                manifest: root.join("docs/package.json"),
            },
        ],
        dependencies: vec![
            dependency("web-next", "web", "runtime", "^16.0.0", Some("16.0.1")),
            dependency("docs-next", "docs", "dev", "^15.5.0", None),
        ],
        warnings: vec![],
    });
    s.set_section(LibrarySection::Packages);
}

pub fn notes_scene(s: &mut State) {
    scene(s);
    let dependency = s.selected().unwrap().dependency.clone().unwrap();
    let id = dependency.id.clone();
    s.engine_query().cache = 2;
    s.engine_query().caption = "Cached public package history / fixture project context".into();
    let result = HttpClient::new(CacheOptions {
        directory: cache_directory(),
        public_only: true,
        offline: true,
        ..Default::default()
    })
    .and_then(|fetch| dependencies::notes(fetch, &dependency, Channel::Stable))
    .map_err(|e| e.to_string());
    s.apply_engine_result(id, result);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_windows_prefixes_are_hidden_only_in_presentation() {
        assert_eq!(
            display_path(Path::new("\\\\?\\C:\\work\\project")),
            "C:\\work\\project"
        );
        assert_eq!(
            display_path(Path::new("\\\\?\\UNC\\server\\share\\project")),
            "\\\\server\\share\\project"
        );
        assert_eq!(display_path(Path::new("/work/project")), "/work/project");
    }
}
