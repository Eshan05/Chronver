use crate::model::{Dataset, EntryKind, LibrarySection, Package, State};
use rune::{Color, El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, icon, row, text};
use rune_kit::{IconButton, IconButtonVariant, Kit, TextInput, glyphs, scroll_bar};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Palette {
    pub open: Option<Session>,
    pub revealed: Option<String>,
}

pub struct Session {
    revision: u64,
    query: TextEdit,
    entries: Vec<Entry>,
    matches: Vec<usize>,
    active: usize,
    total: usize,
}

struct Entry {
    label: String,
    detail: String,
    context: String,
    providers: Vec<releasescout_engine::inventory::Manager>,
    observations: usize,
    keywords: String,
    group: usize,
    target: Target,
}

#[derive(Clone, PartialEq)]
enum Target {
    Package(String, LibrarySection),
    Section(LibrarySection),
    Inventory,
    Theme,
}

const GROUPS: [&str; 4] = ["Software", "Extensions", "Packages", "Actions"];
static INVENTORY: rune::Icon = rune::Icon {
    name: "command-inventory",
    view: 24.,
    body: include_str!("../assets/symbols/package.svg"),
};
static THEME: rune::Icon = rune::Icon {
    name: "command-theme",
    view: 24.,
    body: include_str!("../assets/status/sun.svg"),
};

pub fn section(p: &Package) -> LibrarySection {
    if p.watchlist.is_some() || p.dependency.is_some() {
        LibrarySection::Packages
    } else if p.kind == EntryKind::Extension {
        LibrarySection::Extensions
    } else {
        LibrarySection::Software
    }
}

fn eligible(s: &State, p: &Package) -> bool {
    if !s.show_components && p.component_reason.is_some() {
        return false;
    }
    match section(p) {
        LibrarySection::Software => p.detected == (s.dataset == Dataset::Detected),
        LibrarySection::Extensions => s.dataset == Dataset::Detected && p.detected,
        LibrarySection::Packages => {
            if s.packages.watch.project_mode {
                p.dependency.is_some()
            } else {
                p.watchlist
                    .as_ref()
                    .is_some_and(|w| w.ecosystem == releasescout_engine::watchlist::Ecosystem::Npm)
            }
        }
    }
}

fn entries(s: &State) -> Vec<Entry> {
    let excluded: BTreeSet<_> = s
        .live
        .catalog
        .packages
        .iter()
        .filter(|p| p.tracking == "excluded")
        .map(|p| p.id.as_str())
        .collect();
    let mut groups: BTreeMap<(usize, String, String), Vec<&Package>> = BTreeMap::new();
    for p in s
        .catalog
        .packages
        .iter()
        .filter(|p| eligible(s, p) && !excluded.contains(p.id.as_str()))
    {
        let section = section(p);
        let identity = p.group_key.as_ref().unwrap_or(&p.id);
        let profile = if section == LibrarySection::Extensions {
            crate::extension_ui::profile_key(p).unwrap_or_default()
        } else {
            String::new()
        };
        groups
            .entry((section as usize, identity.clone(), profile))
            .or_default()
            .push(p);
    }
    let mut entries = Vec::new();
    for mut members in groups.into_values() {
        members.sort_by_key(|p| p.display_priority);
        let p = members[0];
        let section = section(p);
        let providers = crate::provider_badge::unique(
            members
                .iter()
                .filter_map(|p| crate::sidebar_filters::provider(p)),
        );
        let category = if let Some(w) = &p.watchlist {
            if w.group.id() == "core" {
                "Core"
            } else {
                "Tooling"
            }
        } else if members.iter().any(|p| {
            p.dependency.as_ref().is_some_and(|d| {
                matches!(
                    d.kind.as_str(),
                    "dependencies" | "optionalDependencies" | "peerDependencies"
                )
            })
        }) {
            "Core"
        } else if members.iter().any(|p| {
            p.dependency
                .as_ref()
                .is_some_and(|d| d.kind == "devDependencies")
        }) {
            "Tooling"
        } else {
            ""
        };
        let context = if section == LibrarySection::Extensions {
            crate::extension_ui::profile_name(p).to_owned()
        } else {
            category.to_owned()
        };
        let detail = if section == LibrarySection::Extensions {
            format!(
                "{} / {}",
                p.provider.and_then(|m| m.editor_name()).unwrap_or("Editor"),
                crate::extension_ui::profile_name(p)
            )
        } else if p.watchlist.is_some() {
            format!(
                "{} / {}",
                crate::provider_badge::label(releasescout_engine::inventory::Manager::Npm),
                category
            )
        } else if let Some(d) = &p.dependency {
            [
                Some(crate::provider_badge::label(
                    releasescout_engine::inventory::Manager::Npm,
                )),
                (!category.is_empty()).then_some(category),
                Some(d.project.as_str()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" / ")
        } else {
            providers
                .iter()
                .copied()
                .map(crate::provider_badge::label)
                .collect::<Vec<_>>()
                .join(" / ")
        };
        let keywords = members
            .iter()
            .map(|p| {
                format!(
                    "{} {} {} {} {}",
                    p.name,
                    p.id,
                    p.manager_label(),
                    p.observation_label,
                    p.reference_id.as_deref().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        entries.push(Entry {
            label: s.display_name(p).into(),
            keywords: format!(
                "{keywords} {detail} {context} {}",
                providers
                    .iter()
                    .copied()
                    .map(crate::provider_badge::label)
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase(),
            detail,
            context,
            providers,
            observations: members.len(),
            group: section as usize,
            target: Target::Package(p.id.clone(), section),
        });
    }
    for section in [
        LibrarySection::Software,
        LibrarySection::Extensions,
        LibrarySection::Packages,
    ] {
        if section == LibrarySection::Extensions && s.dataset != Dataset::Detected {
            continue;
        }
        let label = format!("Go to {}", GROUPS[section as usize]);
        entries.push(Entry {
            keywords: label.to_lowercase(),
            label,
            detail: String::new(),
            context: String::new(),
            providers: vec![],
            observations: 0,
            group: 3,
            target: Target::Section(section),
        });
    }
    for (label, target) in [
        ("Open inventory", Target::Inventory),
        (
            if s.dark {
                "Switch to light mode"
            } else {
                "Switch to dark mode"
            },
            Target::Theme,
        ),
    ] {
        entries.push(Entry {
            label: label.into(),
            keywords: label.to_lowercase(),
            detail: String::new(),
            context: String::new(),
            providers: vec![],
            observations: 0,
            group: 3,
            target,
        });
    }
    entries
}

impl Session {
    fn filter(&mut self) {
        let query = self.query.text.trim().to_lowercase();
        let terms: Vec<_> = query.split_whitespace().collect();
        let mut found: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| terms.iter().all(|term| e.keywords.contains(term)))
            .map(|(i, e)| {
                let name = e.label.to_lowercase();
                let rank = if name == query {
                    0
                } else if name.starts_with(&query) {
                    1
                } else if name.contains(&query) {
                    2
                } else {
                    3
                };
                (i, rank, name)
            })
            .collect();
        found.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| self.entries[a.0].detail.cmp(&self.entries[b.0].detail))
        });
        self.total = found.len();
        let mut group_rank = [u8::MAX; 4];
        for &(i, rank, _) in &found {
            group_rank[self.entries[i].group] = group_rank[self.entries[i].group].min(rank);
        }
        let mut per_group = [0; 4];
        self.matches = found
            .into_iter()
            .filter_map(|(i, _, _)| {
                let group = self.entries[i].group;
                per_group[group] += 1;
                (!query.is_empty() || per_group[group] <= 5).then_some(i)
            })
            .take(60)
            .collect();
        self.matches
            .sort_by_key(|&i| (group_rank[self.entries[i].group], self.entries[i].group));
        self.active = 0;
    }

    fn key(&self, position: usize) -> String {
        format!("command-option-{}", self.matches[position])
    }
}

pub fn can_open(s: &State) -> bool {
    s.custom_icons.picker.is_none()
        && !s.custom_icons.dialog
        && s.description_editor.is_none()
        && !s.ai.modal_open()
        && s.sidebar_actions.command.is_none()
        && s.sidebar_actions.brief.is_none()
        && s.inventory.workspace.review.is_none()
        && !s.inventory.workspace.export_busy
}

pub fn open(s: &mut State, tx: &mut Tx) {
    if !can_open(s) {
        return;
    }
    s.description_hover = None;
    s.reader_controls.menu = None;
    s.inventory.workspace.columns_open = false;
    s.inventory.workspace.context = None;
    s.inventory.picker = false;
    s.reader_controls.inventory_open = None;
    s.sidebar_filters_open = false;
    s.sidebar_sort_open = false;
    s.sidebar_actions.panel = None;
    s.ai.open = None;
    s.chrome.status_hover = false;
    s.chrome.status_pinned = false;
    let mut session = Session {
        revision: s.list_revision,
        query: TextEdit::new(""),
        entries: entries(s),
        matches: vec![],
        active: 0,
        total: 0,
    };
    session.query.focused = true;
    session.filter();
    s.command.open = Some(session);
    tx.focus("command-input", true);
    tx.instant();
}

fn close(s: &mut State) {
    s.command.open = None;
}

pub fn refresh(s: &mut State) {
    if !s
        .command
        .open
        .as_ref()
        .is_some_and(|c| c.revision != s.list_revision)
    {
        return;
    }
    let mut c = s.command.open.take().unwrap();
    let active = c
        .matches
        .get(c.active)
        .map(|&i| c.entries[i].target.clone());
    c.entries = entries(s);
    c.revision = s.list_revision;
    c.filter();
    if let Some(position) = c
        .matches
        .iter()
        .position(|&i| Some(&c.entries[i].target) == active.as_ref())
    {
        c.active = position;
    }
    s.command.open = Some(c);
}

fn activate(s: &mut State, position: usize, tx: &mut Tx) {
    let Some(target) = s.command.open.as_ref().and_then(|c| {
        c.matches
            .get(position)
            .map(|&i| c.entries[i].target.clone())
    }) else {
        return;
    };
    close(s);
    match target {
        Target::Package(id, section) => {
            if !s
                .catalog
                .packages
                .iter()
                .any(|p| p.id == id && eligible(s, p))
                || s.live
                    .catalog
                    .packages
                    .iter()
                    .any(|p| p.id == id && p.tracking == "excluded")
            {
                s.status = "Search result is no longer in the library".into();
                return;
            }
            let previous_reveal = s.command.revealed.take().is_some();
            s.set_section(section);
            if previous_reveal {
                s.set_dataset(s.dataset);
            }
            let visible = crate::sidebar_groups::shown(s).iter().any(|&i| {
                s.rows[i]
                    .members
                    .iter()
                    .any(|&j| s.catalog.packages[j].id == id)
            });
            if !visible {
                // Reveal only this result; never rewrite the user's saved filter preferences.
                s.command.revealed = Some(id.clone());
                s.set_dataset(s.dataset);
            }
            s.select_package(&id);
            s.rebuild_sidebar_groups();
            s.inventory.open = false;
            s.pane.set_open(true);
            s.mark_current_notes_read();
            s.status = s.inventory_summary();
            s.sidebar_scroll.reset();
            if let Some(row) = s.table.cursor().map(str::to_owned) {
                s.table.reveal(row, tx);
            }
            tx.scroll_to_instant("release-notes", 0., 0.);
            tx.focus("package-filter", false);
        }
        Target::Section(section) => {
            let previous_reveal = s.command.revealed.take().is_some();
            s.set_section(section);
            if previous_reveal {
                s.set_dataset(s.dataset);
            }
            s.inventory.open = false;
            s.pane.set_open(true);
            s.status = s.inventory_summary();
            tx.focus("package-filter", true);
        }
        Target::Inventory => {
            s.inventory.open = true;
            tx.focus("workspace-view", false);
        }
        Target::Theme => {
            s.dark = !s.dark;
        }
    }
    tx.instant();
}

fn key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(c) = &mut s.command.open else {
        return false;
    };
    if c.query.preedit().is_some() {
        return false;
    }
    if ev.key == Key::Escape {
        close(s);
        return true;
    }
    if ev.key == Key::Enter {
        if !tx.key_repeat() {
            let active = c.active;
            activate(s, active, tx);
        }
        return true;
    }
    if ev.mods.alt || ev.mods.ctrl || ev.mods.logo || ev.mods.cmd {
        return false;
    }
    let count = c.matches.len();
    let next = match ev.key {
        Key::Down => {
            if count == 0 {
                0
            } else {
                (c.active + 1) % count
            }
        }
        Key::Up => {
            if count == 0 {
                0
            } else {
                (c.active + count - 1) % count
            }
        }
        Key::PageDown => (c.active + 7).min(count.saturating_sub(1)),
        Key::PageUp => c.active.saturating_sub(7),
        Key::Home if c.query.text.is_empty() => 0,
        Key::End if c.query.text.is_empty() => count.saturating_sub(1),
        _ => return false,
    };
    c.active = next;
    if count > 0 {
        tx.scroll_into_view(c.key(next));
    }
    true
}

pub fn reveals(s: &State, p: &Package) -> bool {
    s.command.revealed.as_deref() == Some(p.id.as_str()) && section(p) == s.section
}

pub fn reveal_notice(s: &State, k: &Kit) -> Option<El> {
    s.command.revealed.as_ref()?;
    Some(
        row()
            .key("command-reveal")
            .w_full()
            .gap(6.)
            .px(7.)
            .center_y()
            .child(icon(&glyphs::SEARCH, 13.).color(k.c.text_muted))
            .child(
                text("Opened from search")
                    .font(k.ty.text_xsmall)
                    .color(k.c.text_muted)
                    .grow(1.)
                    .min_w(0.)
                    .truncate(),
            )
            .child(
                IconButton::new(k.glyph.close)
                    .variant(IconButtonVariant::Ghost)
                    .label("Restore filtered list")
                    .help("Restore filtered list")
                    .on_press(|s: &mut State| {
                        s.command.revealed = None;
                        s.set_dataset(s.dataset);
                    })
                    .build(k)
                    .size(22., 22.)
                    .key("command-clear-reveal"),
            ),
    )
}

pub fn trigger(k: &Kit, compact: bool) -> El {
    row()
        .key("global-search")
        .role("button")
        .label("Search library")
        .help("Search library (Ctrl+K)")
        .focusable(true)
        .h(28.)
        .px(8.)
        .gap(8.)
        .center_y()
        .radius(6.)
        .hover(k.c.hover)
        .focus_ring(1., -1., k.c.border_focus)
        .on_press_tx(open)
        .child(icon(&glyphs::SEARCH, 14.).color(k.c.text_muted))
        .children((!compact).then(|| text("Search").font(k.ty.text_xsmall).color(k.c.text_muted)))
        .children((!compact).then(|| {
            text("Ctrl K")
                .font(k.ty.text_xsmall.mono())
                .color(k.c.text_faint)
        }))
}

pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>) {
    let Some(c) = &s.command.open else {
        return;
    };
    let mut results = col().w_full().padding(4.).gap(2.);
    let mut group = None;
    for (position, &index) in c.matches.iter().enumerate() {
        let e = &c.entries[index];
        if group != Some(e.group) {
            results = results.child(
                text(GROUPS[e.group])
                    .font(k.ty.text_xsmall_medium)
                    .color(k.c.text_muted)
                    .px(8.)
                    .pt(10.)
                    .pb(5.),
            );
            group = Some(e.group);
        }
        let glyph = match e.target {
            Target::Package(_, _) => None,
            Target::Section(LibrarySection::Extensions) => Some(&glyphs::FILE_CODE),
            Target::Section(_) => Some(&glyphs::FOLDER),
            Target::Inventory => Some(&INVENTORY),
            Target::Theme => Some(&THEME),
        };
        let mark = if let Target::Package(ref id, _) = e.target {
            s.catalog
                .packages
                .iter()
                .find(|p| &p.id == id)
                .map(|p| crate::ui::mark(p, k, 22.))
                .unwrap_or_else(|| el().size(22., 22.))
        } else {
            el().size(22., 22.)
                .center()
                .child(icon(glyph.unwrap(), 16.).color(k.c.text_muted))
        };
        let label = if e.detail.is_empty() {
            e.label.clone()
        } else {
            format!("{}, {}", e.label, e.detail)
        };
        results =
            results.child(
                row()
                    .key(c.key(position))
                    .role("option")
                    .label(label)
                    .focus_selected(position == c.active)
                    .w_full()
                    .px(8.)
                    .py(8.)
                    .gap(10.)
                    .center_y()
                    .radius(4.)
                    .fill(if position == c.active {
                        k.c.layer_02
                    } else {
                        Color::TRANSPARENT
                    })
                    .preserve_focus_on_press(true)
                    .on_hover(move |s: &mut State| {
                        if let Some(c) = &mut s.command.open {
                            c.active = position;
                        }
                    })
                    .on_press_tx(move |s: &mut State, tx| activate(s, position, tx))
                    .child(mark.a11y_hidden())
                    .child(
                        text(&e.label)
                            .font(k.ty.ui)
                            .color(k.c.text_base)
                            .grow(1.)
                            .min_w(0.)
                            .truncate()
                            .a11y_hidden(),
                    )
                    .children((!e.providers.is_empty() || !e.context.is_empty()).then(|| {
                        row()
                            .key("command-context")
                            .gap(6.)
                            .center_y()
                            .min_w(0.)
                            .max_w(260.)
                            .a11y_hidden()
                            .children((!e.providers.is_empty()).then(|| {
                                crate::provider_badge::strip(&e.providers, e.observations, k)
                            }))
                            .children((!e.context.is_empty()).then(|| {
                                text(&e.context)
                                    .key("command-context-text")
                                    .font(k.ty.text_xsmall)
                                    .color(k.c.text_muted)
                                    .min_w(0.)
                                    .max_w(160.)
                                    .truncate()
                            }))
                    })),
            );
    }
    if c.matches.is_empty() {
        results = results.child(
            text("No results found.")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .py(32.)
                .w_full()
                .center(),
        );
    }
    let input = TextInput::new(&c.query)
        .plain()
        .leading_icon(&glyphs::SEARCH)
        .height(48.)
        .width(None)
        .label("Search library")
        .placeholder("Search software, extensions, packages...")
        .active_descendant((!c.matches.is_empty()).then(|| c.key(c.active)))
        .on_edit(|s: &mut State| &mut s.command.open.as_mut().unwrap().query)
        .on_change(|s: &mut State, tx| {
            if let Some(c) = &mut s.command.open {
                c.filter();
            }
            tx.scroll_to_instant("command-scroll", 0., 0.);
        })
        .on_key(key)
        .build(k)
        .key("command-input")
        .grow(1.)
        .min_w(0.);
    let panel = col()
        .key("command-dialog")
        .role("dialog")
        .label("Search library")
        .w_full()
        .max_w(600.)
        .max_h(440.)
        .min_h(0.)
        .radius(8.)
        .fill(k.c.layer_01)
        .border(1., k.c.border_base)
        .child(
            row().w_full().px(8.).gap(4.).center_y().child(input).child(
                IconButton::new(k.glyph.close)
                    .variant(IconButtonVariant::Ghost)
                    .label("Close search")
                    .on_press(close)
                    .build(k)
                    .size(24., 24.)
                    .key("command-close"),
            ),
        )
        .child(el().h(1.).w_full().fill(k.c.border_base))
        .child(
            scroll_bar(
                k,
                "command-scroll-area",
                "command-scroll",
                col()
                    .key("command-scroll")
                    .role("listbox")
                    .label("Search results")
                    .w_full()
                    .min_h(0.)
                    .shrink()
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(results),
            )
            .w_full()
            .shrink()
            .min_h(0.),
        )
        .child(el().h(1.).w_full().fill(k.c.border_base))
        .child(
            row().w_full().px(12.).py(8.).child(
                text(if c.matches.len() < c.total {
                    format!("{} of {} results", c.matches.len(), c.total)
                } else {
                    crate::rich_select::count_label(c.total, "result")
                })
                .font(k.ty.text_xsmall)
                .color(k.c.text_faint),
            ),
        )
        .shadow(8., 24., Color::BLACK.alpha(0.18))
        .modal(true, close);
    layers.push(
        el().key("command-scrim")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .fill(k.c.scrim)
            .a11y_hidden(),
    );
    layers.push(
        col()
            .key("command-layer")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .px(16.)
            .pt(90.)
            .pb(16.)
            .items_center()
            .child(panel),
    );
}

pub fn scene(s: &mut State) {
    crate::sidebar_info::scene(s);
    crate::extension_ui::scene(s);
    crate::watchlist_ui::scene(s);
    open(s, &mut Tx::default());
}

pub fn query_scene(s: &mut State) {
    if let Some(c) = &mut s.command.open {
        c.query = TextEdit::new("shared");
        c.filter();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Mods, Runtime};

    fn runtime(dark: bool) -> Runtime<State> {
        let mut state = State::new();
        crate::sidebar_info::scene(&mut state);
        crate::extension_ui::scene(&mut state);
        crate::watchlist_ui::scene(&mut state);
        state.set_section(LibrarySection::Software);
        state.dark = dark;
        let mut rt = Runtime::with_text(
            state,
            crate::ui::view,
            (1024., 640.),
            Env {
                system_dark: dark,
                scale: 1.,
            },
            rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES),
        );
        rt.on_key = Some(crate::ui::on_key);
        rt.frame(0.);
        rt
    }

    fn press(rt: &mut Runtime<State>, key: Key) {
        rt.key(KeyEvent {
            key,
            mods: Mods::default(),
        });
        rt.frame(0.);
    }

    fn tap(rt: &mut Runtime<State>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.);
    }

    fn search(rt: &mut Runtime<State>, query: &str) {
        rt.update(open);
        rt.frame(0.);
        press(rt, Key::Text(query.into()));
    }

    #[test]
    fn result_context_reuses_provider_badges_without_badging_profile_or_category_text() {
        use releasescout_engine::inventory::Manager;
        for dark in [true, false] {
            let mut rt = runtime(dark);
            rt.update(|s, _| {
                let p = s
                    .catalog
                    .packages
                    .iter_mut()
                    .find(|p| p.detected && p.name == "GitHub CLI")
                    .unwrap();
                p.group_key = Some("command-multi-provider-fixture".into());
                let mut other = p.clone();
                other.id = "command-winget-fixture".into();
                other.provider = Some(Manager::Winget);
                other.display_priority = u8::MAX;
                s.catalog.packages.push(other);
                for p in &mut s.catalog.packages {
                    if p.provider == Some(Manager::VscodeInsiders)
                        && crate::extension_ui::profile_name(p) == "Work"
                    {
                        p.extension.as_mut().unwrap().profile_name =
                            Some(format!("Work {}", "long profile ".repeat(20)));
                    }
                }
                s.set_dataset(s.dataset);
            });
            for (query, provider, context, multiple) in [
                ("GitHub CLI", Manager::Scoop, None, true),
                (
                    "shared vsc-ins work",
                    Manager::VscodeInsiders,
                    Some("Work "),
                    false,
                ),
                ("next", Manager::Npm, Some("Core"), false),
            ] {
                search(&mut rt, query);
                let c = rt.state.command.open.as_ref().unwrap();
                let e = &c.entries[c.matches[0]];
                assert_eq!(e.providers[0], provider);
                assert_eq!(e.providers.len() > 1, multiple);
                let elements = rt.elements();
                let metadata = elements
                    .iter()
                    .find(|e| e.key.as_deref() == Some("command-context"))
                    .unwrap();
                let inside = |r: &[f32; 4]| {
                    r[0] >= metadata.rect[0] - 0.5
                        && r[0] + r[2] <= metadata.rect[0] + metadata.rect[2] + 0.5
                        && r[1] >= metadata.rect[1] - 0.5
                        && r[1] + r[3] <= metadata.rect[1] + metadata.rect[3] + 0.5
                };
                let key = format!("provider-badge-{}", provider.id());
                assert_eq!(
                    elements
                        .iter()
                        .filter(|e| e
                            .key
                            .as_deref()
                            .is_some_and(|k| k.starts_with("provider-badge-"))
                            && inside(&e.rect))
                        .count(),
                    1
                );
                assert!(
                    elements
                        .iter()
                        .any(|e| e.key.as_deref() == Some(&key) && inside(&e.rect))
                );
                assert_eq!(
                    elements
                        .iter()
                        .any(|e| e.text.as_deref() == Some("+1") && inside(&e.rect)),
                    multiple
                );
                if let Some(prefix) = context {
                    let plain = elements
                        .iter()
                        .find(|e| e.key.as_deref() == Some("command-context-text"))
                        .unwrap();
                    assert!(plain.text.as_deref().unwrap().starts_with(prefix));
                    assert!(inside(&plain.rect));
                } else {
                    assert!(e.context.is_empty());
                }
                press(&mut rt, Key::Escape);
            }
        }
    }

    #[test]
    fn snapshot_groups_products_but_keeps_editor_profiles_and_defers_cargo() {
        let rt = runtime(true);
        let entries = entries(&rt.state);
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.group == 0 && e.label == "GitHub CLI")
                .count(),
            1
        );
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.group == 1 && e.keywords.contains("vendor.shared"))
                .count(),
            releasescout_engine::inventory::Manager::EDITORS.len() * 2
        );
        assert!(entries.iter().any(|e| e.label == "next" && e.group == 2));
        assert!(!entries.iter().any(|e| e.label == "serde"));
        assert!(!entries.iter().any(|e| e.label == "Angular CLI"));
        assert!(
            entries
                .iter()
                .any(|e| e.detail.contains("Insiders") && e.detail.contains("Work"))
        );
    }

    #[test]
    fn global_search_keyboard_typing_navigation_and_focus_return_work_in_both_themes() {
        for dark in [true, false] {
            let mut rt = runtime(dark);
            tap(&mut rt, "global-search");
            assert!(rt.find("command-dialog").is_some());
            assert!(
                rt.elements()
                    .iter()
                    .any(|e| e.key.as_deref() == Some("input") && e.focused)
            );
            let before = rt.state.command.open.as_ref().unwrap().active;
            press(&mut rt, Key::Down);
            assert_eq!(rt.state.command.open.as_ref().unwrap().active, before + 1);
            press(&mut rt, Key::Up);
            assert_eq!(rt.state.command.open.as_ref().unwrap().active, before);
            press(&mut rt, Key::Up);
            let c = rt.state.command.open.as_ref().unwrap();
            assert_eq!(c.active, c.matches.len() - 1);
            let active_key = c.key(c.active);
            rt.frame(1.);
            let option = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(&active_key))
                .unwrap();
            let panel = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("command-dialog"))
                .unwrap();
            assert!(
                option.rect[1] >= panel.rect[1]
                    && option.rect[1] + option.rect[3] <= panel.rect[1] + panel.rect[3]
            );
            press(&mut rt, Key::Escape);
            assert!(rt.state.command.open.is_none());
            assert!(
                rt.elements()
                    .iter()
                    .any(|e| e.key.as_deref() == Some("global-search") && e.focused)
            );
            press(&mut rt, Key::Command('k'));
            press(&mut rt, Key::Text("  NeXt  ".into()));
            let c = rt.state.command.open.as_ref().unwrap();
            assert_eq!(c.entries[c.matches[0]].label, "next");
            assert!(rt.state.query.text.is_empty());
            press(&mut rt, Key::Enter);
            assert!(rt.state.command.open.is_none());
            assert!(rt.state.section == LibrarySection::Packages);
            assert_eq!(rt.state.selected().unwrap().name, "next");
            assert!(!rt.state.inventory.open);
            press(&mut rt, Key::Command('f'));
            press(&mut rt, Key::Text("zod".into()));
            assert_eq!(rt.state.query.text, "zod");
            assert!(rt.state.command.open.is_none());
        }
    }

    #[test]
    fn opening_filtered_software_temporarily_reveals_it_without_changing_preferences() {
        let mut rt = runtime(true);
        rt.update(|s, _| {
            s.query = TextEdit::new("does-not-match");
            s.activity.data.sidebar_filters[0].update = crate::sidebar_filters::Update::Current;
            s.refilter();
        });
        let facets = rt.state.activity.data.sidebar_filters.clone();
        search(&mut rt, "GitHub CLI");
        press(&mut rt, Key::Enter);
        assert_eq!(rt.state.selected().unwrap().name, "GitHub CLI");
        assert!(rt.find("command-reveal").is_some());
        assert_eq!(rt.state.activity.data.sidebar_filters, facets);
        assert_eq!(rt.state.query.text, "does-not-match");
        tap(&mut rt, "command-clear-reveal");
        assert!(rt.state.command.revealed.is_none());
        assert!(rt.state.visible.is_empty());
        assert_eq!(rt.state.activity.data.sidebar_filters, facets);
    }

    #[test]
    fn profile_search_reveals_the_exact_membership_and_respects_later_profile_changes() {
        let mut rt = runtime(true);
        rt.update(|s, _| {
            s.extensions.editor = Some(releasescout_engine::inventory::Manager::Vscode);
            s.set_section(LibrarySection::Extensions);
        });
        search(&mut rt, "shared insiders work");
        assert_eq!(rt.state.command.open.as_ref().unwrap().matches.len(), 1);
        press(&mut rt, Key::Enter);
        let selected = rt.state.selected().unwrap();
        assert_eq!(
            selected.provider,
            Some(releasescout_engine::inventory::Manager::VscodeInsiders)
        );
        assert_eq!(crate::extension_ui::profile_name(selected), "Work");
        assert!(rt.state.command.revealed.is_some());
        assert_eq!(
            rt.state.extensions.editor,
            Some(releasescout_engine::inventory::Manager::Vscode)
        );
        assert!(crate::sidebar_groups::shown(&rt.state).iter().any(|&i| {
            rt.state.rows[i]
                .members
                .iter()
                .any(|&j| rt.state.catalog.packages[j].id == selected.id)
        }));
        rt.update(|s, tx| s.refresh_extension_filter(tx));
        rt.frame(0.);
        assert!(rt.state.command.revealed.is_none());
        assert!(
            rt.state
                .rows
                .iter()
                .all(|r| rt.state.catalog.packages[r.package].provider
                    == Some(releasescout_engine::inventory::Manager::Vscode))
        );
    }

    #[test]
    fn empty_results_outside_dismissal_actions_and_modal_guards_are_safe() {
        let mut rt = runtime(true);
        search(&mut rt, "no-such-item-zzzz");
        press(&mut rt, Key::Down);
        press(&mut rt, Key::Enter);
        assert!(rt.state.command.open.as_ref().unwrap().matches.is_empty());
        rt.pointer_down(4., 4.);
        rt.pointer_up(4., 4.);
        rt.frame(0.);
        assert!(rt.state.command.open.is_none());
        assert!(rt.state.dark);
        search(&mut rt, "switch to light");
        press(&mut rt, Key::Enter);
        assert!(!rt.state.dark);
        assert!(rt.state.command.open.is_none());
        rt.update(|s, _| s.open_description_editor());
        rt.frame(0.);
        assert!(rt.state.description_editor.is_some());
        press(&mut rt, Key::Command('k'));
        assert!(rt.state.command.open.is_none());
        assert!(rt.state.description_editor.is_some());
    }

    #[test]
    fn exact_names_rank_ahead_of_other_groups_and_results_are_bounded() {
        let mut rt = runtime(true);
        rt.update(|s, _| {
            s.catalog
                .packages
                .iter_mut()
                .find(|p| p.detected && p.name == "GitHub CLI")
                .unwrap()
                .name = "next helper".into();
        });
        search(&mut rt, "next");
        let c = rt.state.command.open.as_ref().unwrap();
        assert_eq!(c.entries[c.matches[0]].label, "next");
        assert_eq!(c.entries[c.matches[0]].group, 2);
        rt.update(|s, _| *s = State::demo());
        rt.frame(0.);
        search(&mut rt, "");
        let c = rt.state.command.open.as_ref().unwrap();
        assert!(c.matches.len() < c.total);
        assert!(c.matches.len() <= 20);
    }

    #[test]
    fn selecting_a_late_result_reopens_and_scrolls_the_sidebar_into_view() {
        let mut rt = runtime(true);
        rt.update(|s, _| {
            *s = State::demo();
            s.pane.set_open(false);
        });
        rt.frame(0.);
        search(&mut rt, "yarn");
        assert!(!rt.state.pane.is_open());
        press(&mut rt, Key::Enter);
        rt.frame(1.);
        assert!(rt.state.pane.is_open());
        assert_eq!(rt.state.selected().unwrap().id, "yarn");
        let row = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("yarn"))
            .unwrap();
        let viewport = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("viewport"))
            .unwrap();
        assert!(
            row.rect[1] >= viewport.rect[1] - 1.
                && row.rect[1] + row.rect[3] <= viewport.rect[1] + viewport.rect[3] + 1.,
            "row {:?}, viewport {:?}",
            row.rect,
            viewport.rect
        );
    }

    #[test]
    fn disappearing_results_are_not_replaced_with_a_different_package() {
        let mut rt = runtime(true);
        search(&mut rt, "next");
        rt.update(|s, _| {
            s.catalog.packages.retain(|p| p.name != "next");
            s.set_dataset(s.dataset);
        });
        rt.frame(0.);
        let before = rt.state.selected().map(|p| p.id.clone());
        press(&mut rt, Key::Enter);
        assert!(rt.state.command.open.is_none());
        assert_eq!(rt.state.selected().map(|p| p.id.clone()), before);
        assert_eq!(rt.state.status, "Search result is no longer in the library");
    }

    #[test]
    fn background_inventory_changes_refresh_an_open_search_without_losing_its_query() {
        let mut rt = runtime(true);
        search(&mut rt, "new fixture");
        assert!(rt.state.command.open.as_ref().unwrap().matches.is_empty());
        rt.update(|s, _| {
            let p = s
                .catalog
                .packages
                .iter_mut()
                .find(|p| p.detected && p.name == "GitHub CLI")
                .unwrap();
            p.name = "New Fixture".into();
            s.set_dataset(s.dataset);
            refresh(s);
        });
        rt.frame(0.);
        let c = rt.state.command.open.as_ref().unwrap();
        assert_eq!(c.query.text, "new fixture");
        assert_eq!(c.entries[c.matches[0]].label, "New Fixture");
    }

    #[test]
    fn hidden_support_and_excluded_items_do_not_leak_into_results() {
        let mut s = State::new();
        crate::sidebar_info::scene(&mut s);
        let p = s
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.name == "GitHub CLI" && p.detected)
            .unwrap();
        p.component_reason = Some("Support fixture".into());
        let id = p.id.clone();
        assert!(
            !entries(&s)
                .iter()
                .any(|e| matches!(&e.target, Target::Package(target, _) if target == &id))
        );
        s.show_components = true;
        assert!(
            entries(&s)
                .iter()
                .any(|e| matches!(&e.target, Target::Package(target, _) if target == &id))
        );
        if let Some(profile) = s.live.catalog.packages.iter_mut().find(|p| p.id == id) {
            profile.tracking = "excluded".into();
        }
        assert!(
            !entries(&s)
                .iter()
                .any(|e| matches!(&e.target, Target::Package(target, _) if target == &id))
        );
    }
}
