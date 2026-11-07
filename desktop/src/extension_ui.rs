use crate::{
    live::cache_directory,
    model::{Package, State},
};
use crate::{
    provider_badge,
    rich_select::{OptionRow, RichSelect, count_label},
};
use releasescout_engine::{
    Channel,
    extension_updates::{self, Installation, Status},
    extensions::{self, Mode},
    http::{CacheOptions, HttpClient},
    inventory::{Manager, ProcessRunner},
};
use rune::{El, Latest, col, icon, row, text};
use rune_kit::{Button, ButtonVariant, Kit, SegmentedControl, glyphs};
use std::collections::HashMap;

static REFRESH: rune::Icon = rune::Icon {
    name: "check-extension-updates",
    view: 24.,
    body: include_str!("../assets/status/refresh-cw.svg"),
};

struct Update {
    key: String,
    editor: Manager,
    installation: Installation,
    status: Status,
}

#[derive(Default)]
pub struct Filter {
    pub editor: Option<Manager>,
    pub profile: Option<String>,
    editor_open: bool,
    profile_open: bool,
    updates: HashMap<String, Update>,
    update_work: Latest<Vec<Update>>,
}

impl Update {
    fn matches(&self, p: &Package) -> bool {
        p.provider == Some(self.editor)
            && p.installed.as_deref() == Some(&self.installation.version)
            && p.extension.as_ref().is_some_and(|e| {
                e.id == self.installation.id
                    && e.profile_id.as_deref() == Some(&self.installation.profile)
                    && e.root == self.installation.root
            })
    }
}

fn update_status<'a>(s: &'a State, p: &Package) -> Option<&'a Status> {
    p.extension.as_ref()?;
    s.extensions
        .updates
        .get(&p.id)
        .filter(|update| update.matches(p))
        .map(|u| &u.status)
}

pub fn has_update(s: &State, p: &Package) -> bool {
    matches!(update_status(s, p), Some(Status::Available(_)))
}

pub fn update_label(s: &State, p: &Package, k: &Kit) -> Option<El> {
    let (label, detail, color) = match update_status(s, p)? {
        Status::Available(v) => (
            format!("Gallery update: {v}"),
            "A newer compatible release was found in this editor's gallery. Editor policies may still apply.",
            k.c.success.fg,
        ),
        Status::NoNewerCompatibleRelease => (
            "No newer compatible release".into(),
            "No newer compatible version was found in the last gallery check.",
            k.c.text_muted,
        ),
        Status::Unverified(reason) => ("Update unverified".into(), *reason, k.c.text_muted),
    };
    Some(
        text(label)
            .font(k.ty.text_xsmall)
            .color(color)
            .help(detail)
            .max_w(280.)
            .min_w(0.)
            .truncate()
            .key("extension-update-status"),
    )
}
pub fn profile_key(p: &Package) -> Option<String> {
    let e = p.extension.as_ref()?;
    let manager = p.provider?;
    let identity = e
        .profile_id
        .as_deref()
        .or_else(|| p.observation.as_ref().and_then(|o| o.root.as_deref()))
        .or_else(|| p.observation.as_ref().map(|o| o.scope.as_str()))
        .unwrap_or("__default__profile__");
    Some(crate::activity::hash((manager.id(), identity)))
}
pub fn profile_name(p: &Package) -> &str {
    p.extension
        .as_ref()
        .and_then(|e| e.profile_name.as_deref())
        .or_else(|| {
            p.observation
                .as_ref()
                .map(|o| o.scope.strip_prefix("profile: ").unwrap_or(&o.scope))
        })
        .unwrap_or("Default")
}
pub fn membership_label(p: &Package) -> String {
    format!(
        "{} / {} / {}",
        p.provider
            .map(crate::provider_badge::label)
            .unwrap_or("Editor"),
        profile_name(p),
        p.installed.as_deref().unwrap_or("Unresolved")
    )
}
impl Filter {
    pub fn set_update_waker(&self, waker: rune::Waker) {
        self.update_work.set_waker(waker);
    }

    pub fn matches(&self, p: &Package) -> bool {
        self.editor.is_none_or(|m| p.provider == Some(m))
            && self
                .profile
                .as_ref()
                .is_none_or(|key| profile_key(p).as_ref() == Some(key))
    }
}
struct ProfileSummary<'a> {
    id: String,
    label: String,
    package: &'a Package,
    extensions: std::collections::BTreeSet<String>,
}

fn profiles(s: &State) -> Vec<ProfileSummary<'_>> {
    let mut result = std::collections::BTreeMap::new();
    for p in s
        .catalog
        .packages
        .iter()
        .filter(|p| p.detected && p.extension.is_some())
    {
        let Some(key) = profile_key(p) else {
            continue;
        };
        let name = if s.extensions.editor.is_none() {
            format!(
                "{} / {}",
                p.provider
                    .and_then(Manager::editor_name)
                    .unwrap_or("Editor"),
                profile_name(p)
            )
        } else {
            profile_name(p).to_owned()
        };
        let summary = result.entry(key.clone()).or_insert_with(|| ProfileSummary {
            id: key,
            label: name,
            package: p,
            extensions: Default::default(),
        });
        summary
            .extensions
            .insert(p.extension.as_ref().unwrap().id.to_ascii_lowercase());
    }
    let mut items: Vec<_> = result.into_iter().collect();
    let names: Vec<_> = items
        .iter()
        .map(|(_, p)| (p.package.provider, p.label.clone()))
        .collect();
    for (_, p) in &mut items {
        if names
            .iter()
            .filter(|(m, n)| *m == p.package.provider && *n == p.label)
            .count()
            > 1
        {
            p.label = format!("{} ({})", p.label, &p.id[..6]);
        }
    }
    items.sort_by_cached_key(|(_, p)| p.label.to_lowercase());
    items.into_iter().map(|(_, p)| p).collect()
}
fn counts(
    items: &[ProfileSummary<'_>],
    editor: Option<Manager>,
    profile: Option<&str>,
) -> (usize, usize) {
    let mut profiles = 0;
    let mut extensions = std::collections::BTreeSet::new();
    for p in items.iter().filter(|p| {
        editor.is_none_or(|m| p.package.provider == Some(m)) && profile.is_none_or(|id| p.id == id)
    }) {
        profiles += 1;
        extensions.extend(&p.extensions);
    }
    (profiles, extensions.len())
}

fn field_value(name: &str, editor: Option<Manager>, k: &Kit) -> El {
    row()
        .gap(8.)
        .center_y()
        .child(
            editor
                .map(|m| provider_badge::symbol_sized(m, k, 16.))
                .unwrap_or_else(|| icon(&glyphs::FOLDER, 16.).color(k.c.text_muted)),
        )
        .child(
            text(name)
                .font(k.ty.ui_compact)
                .color(k.c.text_base)
                .grow(1.)
                .min_w(0.)
                .truncate(),
        )
}

fn editor_option(items: &[ProfileSummary<'_>], editor: Option<Manager>, k: &Kit) -> OptionRow {
    let name = editor
        .and_then(Manager::editor_name)
        .unwrap_or("All editors");
    let (profiles, extensions) = counts(items, editor, None);
    let description = format!(
        "{} / {}",
        count_label(profiles, "profile"),
        count_label(extensions, "extension")
    );
    let count = row()
        .h(18.)
        .px(4.)
        .center_y()
        .radius(4.)
        .fill(k.c.layer_02)
        .border(0.5, k.c.border_base)
        .child(
            text(count_label(profiles, "profile"))
                .font(k.ty.small_medium)
                .color(k.c.text_muted),
        );
    OptionRow {
        id: editor.map(Manager::id).unwrap_or("all").into(),
        label: name.into(),
        description,
        content: row()
            .gap(10.)
            .center_y()
            .child(
                editor
                    .map(|m| provider_badge::symbol_sized(m, k, 20.))
                    .unwrap_or_else(|| icon(&glyphs::FILE_CODE, 20.).color(k.c.text_muted)),
            )
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(3.)
                    .child(
                        text(name)
                            .font(k.ty.ui_medium)
                            .color(k.c.text_base)
                            .truncate(),
                    )
                    .child(
                        text(count_label(extensions, "extension"))
                            .font(k.ty.text_xsmall)
                            .color(k.c.text_muted),
                    ),
            )
            .child(count),
    }
}

fn profile_option(
    items: &[ProfileSummary<'_>],
    filter: Option<Manager>,
    id: &str,
    label: &str,
    p: Option<&Package>,
    k: &Kit,
) -> OptionRow {
    let editor = p.and_then(|p| p.provider).or(filter);
    let name = p.map(profile_name).unwrap_or("All profiles");
    let (profiles, extensions) = counts(items, filter, (id != "all").then_some(id));
    let description = if p.is_some() {
        format!(
            "{} / {}",
            editor.and_then(Manager::editor_name).unwrap_or("Editor"),
            count_label(extensions, "extension")
        )
    } else {
        format!(
            "{} / {}",
            count_label(profiles, "profile"),
            count_label(extensions, "extension")
        )
    };
    OptionRow {
        id: id.into(),
        label: label.into(),
        description: description.clone(),
        content: col()
            .gap(4.)
            .min_w(0.)
            .child(
                text(name)
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base)
                    .truncate(),
            )
            .child(
                row()
                    .gap(6.)
                    .center_y()
                    .children(editor.map(|m| provider_badge::badge(m, k)))
                    .child(
                        text(if p.is_some() {
                            count_label(extensions, "extension")
                        } else {
                            description
                        })
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_muted)
                        .grow(1.)
                        .min_w(0.)
                        .truncate(),
                    ),
            ),
    }
}

pub fn filters(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    let items = profiles(s);
    let mut editors = vec![editor_option(&items, None, k)];
    editors.extend(Manager::EDITORS.map(|m| editor_option(&items, Some(m), k)));
    let editor = RichSelect {
        key: "extension-editor",
        label: "Filter extensions by editor",
        current: s.extensions.editor.map(Manager::id).unwrap_or("all"),
        open: s.extensions.editor_open,
        popup_width: 320.,
        value: field_value(
            s.extensions
                .editor
                .and_then(Manager::editor_name)
                .unwrap_or("All editors"),
            s.extensions.editor,
            k,
        ),
        options: editors,
        on_toggle: |s: &mut State| s.extensions.editor_open = !s.extensions.editor_open,
        on_close: |s: &mut State| s.extensions.editor_open = false,
        on_choose: |s: &mut State, id, tx| {
            s.extensions.editor = Manager::EDITORS.into_iter().find(|m| m.id() == id);
            s.extensions.profile = None;
            s.refresh_extension_filter(tx);
        },
    }
    .build(k, layers);
    let mut options = vec![profile_option(
        &items,
        s.extensions.editor,
        "all",
        "All profiles",
        None,
        k,
    )];
    for p in items.iter().filter(|p| {
        s.extensions
            .editor
            .is_none_or(|m| p.package.provider == Some(m))
    }) {
        options.push(profile_option(
            &items,
            s.extensions.editor,
            &p.id,
            &p.label,
            Some(p.package),
            k,
        ));
    }
    let selected = s
        .extensions
        .profile
        .as_ref()
        .and_then(|id| items.iter().find(|p| &p.id == id).map(|p| p.package));
    let profile = RichSelect {
        key: "extension-profile",
        label: "Filter extensions by profile",
        current: s.extensions.profile.as_deref().unwrap_or("all"),
        open: s.extensions.profile_open,
        popup_width: 320.,
        value: field_value(
            selected.map(profile_name).unwrap_or("All profiles"),
            selected.and_then(|p| p.provider),
            k,
        ),
        options,
        on_toggle: |s: &mut State| s.extensions.profile_open = !s.extensions.profile_open,
        on_close: |s: &mut State| s.extensions.profile_open = false,
        on_choose: |s: &mut State, id, tx| {
            s.extensions.profile = (id != "all").then(|| id.to_owned());
            s.refresh_extension_filter(tx);
        },
    }
    .build(k, layers);
    let checking = s.extensions.update_work.is_pending();
    let mut available = 0;
    let mut unverified = 0;
    let mut checked = 0;
    for p in s
        .catalog
        .packages
        .iter()
        .filter(|p| p.extension.is_some() && s.extensions.matches(p))
    {
        if let Some(status) = update_status(s, p) {
            checked += 1;
            available += usize::from(matches!(status, Status::Available(_)));
            unverified += usize::from(matches!(status, Status::Unverified(_)));
        }
    }
    let summary = if checking {
        Some("Checking...".to_owned())
    } else if checked > 0 {
        Some(if unverified > 0 {
            format!("{available} updates / {unverified} unverified")
        } else {
            format!("{available} updates")
        })
    } else {
        None
    };
    col()
        .w_full()
        .gap(6.)
        .child(editor)
        .child(profile)
        .child(
            row().min_h(30.).gap(6.).center_y()
                .child(Button::new("Check updates")
                    .icon(&REFRESH)
                    .variant(ButtonVariant::Outline)
                    .disabled(checking || items.is_empty())
                    .on_press(|s: &mut State| s.start_extension_updates())
                    .build(k)
                    .help("Check this editor and profile selection without opening an editor or installing anything")
                    .key("extension-check-updates"))
                .children(summary.map(|label| text(&label).color(k.c.text_muted).font(k.ty.text_xsmall)
                    .grow(1.).min_w(0.).truncate().help_when_truncated(label))),
        )
        .key("extension-filters")
}

pub fn controls(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    let p = s.selected().unwrap();
    let q = &s.live.sessions[&p.id];
    crate::reader_controls::area("extension-controls").child(
        row()
            .w_full()
            .flex_wrap()
            .gap(8.0)
            .center_y()
            .child(
                SegmentedControl::new(
                    &["Installed", "Published"],
                    Some(usize::from(q.extension_published)),
                )
                .width(Some(184.0))
                .label("Extension notes source")
                .on_pick(|s: &mut State, i, _| s.engine_query().extension_published = i == 1)
                .build(k)
                .key("extension-source"),
            )
            .child(crate::reader_controls::actions(
                s,
                k,
                layers,
                q.extension_published,
            )),
    )
}
impl State {
    fn start_extension_updates(&mut self) {
        if self.extensions.update_work.is_pending() {
            return;
        }
        let targets: Vec<_> = self
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected && self.extensions.matches(p))
            .filter_map(|p| {
                let info = p.extension.as_ref()?;
                Some((
                    p.id.clone(),
                    p.provider?,
                    Installation {
                        id: info.id.clone(),
                        version: p.installed.clone()?,
                        profile: info.profile_id.clone()?,
                        root: info.root.clone(),
                    },
                ))
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        self.extensions.update_work.request((), move |_| {
            let client = HttpClient::new(CacheOptions {
                directory: cache_directory(),
                ..Default::default()
            });
            let runner = ProcessRunner {
                timeout: std::time::Duration::from_secs(10),
                ..Default::default()
            };
            let mut results = Vec::new();
            for editor in Manager::EDITORS {
                let (keys, installs): (Vec<_>, Vec<_>) = targets
                    .iter()
                    .filter(|(_, provider, _)| *provider == editor)
                    .map(|(key, _, install)| (key.clone(), install.clone()))
                    .unzip();
                if installs.is_empty() {
                    continue;
                }
                let statuses = match client.as_ref() {
                    Ok(client) => extension_updates::check(editor, &installs, &runner, client)
                        .unwrap_or_else(|_| {
                            vec![
                                Status::Unverified("Could not verify the editor or its gallery");
                                installs.len()
                            ]
                        }),
                    Err(_) => vec![
                        Status::Unverified("Could not connect to the editor gallery");
                        installs.len()
                    ],
                };
                results.extend(keys.into_iter().zip(installs).zip(statuses).map(
                    |((key, installation), status)| Update {
                        key,
                        editor,
                        installation,
                        status,
                    },
                ));
            }
            results
        });
    }

    pub fn poll_extension_updates(&mut self) -> bool {
        let Some(results) = self.extensions.update_work.take() else {
            return false;
        };
        self.extensions
            .updates
            .extend(results.into_iter().map(|u| (u.key.clone(), u)));
        true
    }

    pub fn normalize_extension_filter(&mut self) {
        if self.extensions.profile.as_ref().is_some_and(|key| {
            !self.catalog.packages.iter().any(|p| {
                p.detected && p.extension.is_some() && profile_key(p).as_ref() == Some(key)
            })
        }) {
            self.extensions.profile = None;
        }
    }

    pub fn refresh_extension_filter(&mut self, tx: &mut rune::Tx) {
        self.command.revealed = None;
        let selected = self
            .selected()
            .and_then(|p| p.extension.as_ref().map(|e| e.id.to_ascii_lowercase()));
        self.observation_picker = false;
        self.set_dataset(self.dataset);
        if let Some(p) = self.rows.iter().find_map(|r| {
            let p = &self.catalog.packages[r.package];
            p.extension
                .as_ref()
                .filter(|e| {
                    selected
                        .as_ref()
                        .is_some_and(|id| e.id.eq_ignore_ascii_case(id))
                })
                .map(|_| p.id.clone())
        }) {
            self.select_package(&p);
        }
        self.mark_current_notes_read();
        self.status = self.inventory_summary();
        tx.scroll_to_instant("viewport", 0., 0.);
        tx.scroll_to_instant("release-notes", 0., 0.);
        tx.instant();
    }

    pub fn fetch_extension_notes(&mut self) {
        if let Some(id) = self.selected().map(|p| p.id.clone()) {
            self.capture_note_feed(&id, false);
        }
        let Some(p) = self.selected() else {
            return;
        };
        let Some(info) = p.extension.clone() else {
            return;
        };
        let id = p.id.clone();
        let installed = p.installed.clone().unwrap_or_default();
        let q = &self.live.sessions[&id];
        let mode = if q.extension_published {
            Mode::Published
        } else {
            Mode::Installed
        };
        let channel = [Channel::Stable, Channel::Preview, Channel::All][q.channel];
        let options = CacheOptions {
            directory: cache_directory(),
            offline: q.cache == 2,
            refresh: q.cache == 1,
            ..Default::default()
        };
        self.live.history.cancel();
        self.live.active = Some(id.clone());
        let q = self.live.sessions.get_mut(&id).unwrap();
        q.error = None;
        q.next = None;
        q.report = None;
        q.shown = 50;
        q.caption = if mode == Mode::Installed {
            "Installed extension notes"
        } else {
            "Published extension changelog"
        }
        .into();
        self.catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .releases
            .clear();
        self.live.work.request(id, move |id| {
            let result = HttpClient::new(options)
                .and_then(|fetcher| {
                    extensions::notes(&fetcher, &id, &info.id, &installed, &info, mode, channel)
                })
                .map_err(|e| e.to_string());
            (id, result)
        });
    }
}

pub fn scene(s: &mut State) {
    for (i, manager) in Manager::EDITORS.into_iter().enumerate() {
        let scan = releasescout_engine::inventory::parse(manager, false, &serde_json::json!({"profiles":[
            {"id":"default","name":"Default","root":format!("Z:/editors/{}/User",manager.id()),"installed":format!("vendor.shared@{}.0.0\nvendor.{}@1.0.0",i+1,manager.id())},
            {"id":"work","name":"Work","icon":"beaker","root":format!("Z:/editors/{}/User/profiles/work",manager.id()),"installed":format!("vendor.shared@{}.1.0",i+1)}
        ]}).to_string()).unwrap();
        s.accept_scan(scan);
    }
    s.set_section(crate::model::LibrarySection::Extensions);
    let id = s
        .catalog
        .packages
        .iter()
        .find(|p| {
            p.extension
                .as_ref()
                .is_some_and(|e| e.id == "vendor.shared")
        })
        .unwrap()
        .id
        .clone();
    s.select_package(&id);
}

pub fn menu_scene(s: &mut State, editor: bool) {
    scene(s);
    s.extensions.editor_open = editor;
    s.extensions.profile_open = !editor;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gallery_result_is_visible_only_for_its_captured_installation() {
        let mut s = State::demo();
        scene(&mut s);
        let p = s.selected().unwrap();
        let info = p.extension.as_ref().unwrap();
        let key = p.id.clone();
        let record = Update {
            key: key.clone(),
            editor: p.provider.unwrap(),
            installation: Installation {
                id: info.id.clone(),
                version: p.installed.clone().unwrap(),
                profile: info.profile_id.clone().unwrap(),
                root: info.root.clone(),
            },
            status: Status::Available("9.0.0".into()),
        };
        s.extensions.updates.insert(key.clone(), record);
        let mut rt = rune::Runtime::with_text(
            s,
            crate::ui::view,
            (1024., 640.),
            rune::Env {
                system_dark: false,
                scale: 1.,
            },
            rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES),
        );
        rt.frame(0.);
        assert!(rt.find("extension-check-updates").is_some());
        assert!(rt.find("sidebar-extension-update").is_some());
        assert!(rt.find("extension-update-status").is_some());
        assert!(!rt.state.extensions.update_work.is_pending());
        rt.update(|s, _| {
            s.catalog
                .packages
                .iter_mut()
                .find(|p| p.id == key)
                .unwrap()
                .installed = Some("9.0.0".into());
        });
        rt.frame(1.);
        assert!(rt.find("extension-update-status").is_none());
        assert!(rt.find("sidebar-extension-update").is_none());
    }

    #[test]
    fn rich_option_counts_are_distinct_extensions_not_membership_totals() {
        let mut s = State::new();
        scene(&mut s);
        let items = profiles(&s);
        assert_eq!(counts(&items, None, None), (6, 4));
        assert_eq!(counts(&items, Some(Manager::Vscode), None), (2, 2));
        let profile = items
            .iter()
            .find(|p| {
                p.package.provider == Some(Manager::Vscode) && profile_name(p.package) == "Work"
            })
            .unwrap();
        assert_eq!(counts(&items, None, Some(&profile.id)), (1, 1));
        let k = crate::theme::kit(true);
        let option = editor_option(&items, Some(Manager::Vscode), &k);
        assert_eq!(option.description, "2 profiles / 2 extensions");
        let option = profile_option(
            &items,
            None,
            &profile.id,
            &profile.label,
            Some(profile.package),
            &k,
        );
        assert_eq!(option.description, "VS Code / 1 extension");
    }
}
