use crate::model::{LibrarySection, PackageRow, State};
use rune::{El, col, icon, row, text};
use rune_kit::{Kit, SortDir, glyphs};
use std::collections::{BTreeMap, BTreeSet};

pub enum Item {
    Header {
        id: String,
        label: String,
        count: usize,
        open: bool,
        package: usize,
    },
    Entry {
        visible: usize,
    },
}
#[derive(Default)]
pub struct Groups {
    pub items: Vec<Item>,
    pub shown: Vec<usize>,
    closed: BTreeSet<String>,
    search_closed: BTreeSet<String>,
    search_context: String,
}

fn group(s: &State, r: &PackageRow) -> (String, String, u8) {
    if s.section == LibrarySection::Software {
        let providers = crate::provider_badge::unique(
            r.members
                .iter()
                .filter_map(|&i| s.catalog.packages[i].provider),
        );
        return match providers.as_slice() {
            [provider] => (
                format!("sidebar-group-software-provider-{}", provider.id()),
                crate::provider_badge::label(*provider).into(),
                0,
            ),
            [] => (
                "sidebar-group-software-unrecorded".into(),
                "Unrecorded provider".into(),
                2,
            ),
            _ => (
                "sidebar-group-software-multiple".into(),
                "Multiple providers".into(),
                1,
            ),
        };
    }
    if s.section == LibrarySection::Extensions {
        let p = &s.catalog.packages[r.package];
        let key = crate::extension_ui::profile_key(p).unwrap_or_else(|| "unknown".into());
        let editor = p.provider.and_then(|m| m.editor_name()).unwrap_or("Editor");
        return (
            format!("sidebar-group-extensions-{key}"),
            format!("{editor} / {}", crate::extension_ui::profile_name(p)),
            0,
        );
    }
    let p = &s.catalog.packages[r.package];
    let category = if let Some(w) = &p.watchlist {
        Some(w.group.id())
    } else if r.members.iter().any(|&i| {
        s.catalog.packages[i].dependency.as_ref().is_some_and(|d| {
            matches!(
                d.kind.as_str(),
                "dependencies" | "optionalDependencies" | "peerDependencies"
            )
        })
    }) {
        Some("core")
    } else if r.members.iter().any(|&i| {
        s.catalog.packages[i]
            .dependency
            .as_ref()
            .is_some_and(|d| d.kind == "devDependencies")
    }) {
        Some("tooling")
    } else {
        None
    };
    match category {
        Some("core") => ("sidebar-group-packages-core".into(), "Core".into(), 0),
        Some("tooling") => ("sidebar-group-packages-tooling".into(), "Tooling".into(), 1),
        _ => ("sidebar-group-packages-other".into(), "Other".into(), 2),
    }
}

impl State {
    pub fn rebuild_sidebar_groups(&mut self) {
        if !grouped(self) {
            self.sidebar_groups.items.clear();
            self.sidebar_groups.shown.clear();
            self.dismiss_hidden_sidebar_overlays();
            return;
        }
        let context = format!(
            "{}:{}",
            self.section as usize,
            self.query.text.trim().to_lowercase()
        );
        if context != self.sidebar_groups.search_context {
            self.sidebar_groups.search_context = context;
            self.sidebar_groups.search_closed.clear();
        }
        let mut groups: BTreeMap<(u8, String, String), Vec<usize>> = BTreeMap::new();
        for (position, &i) in self.visible.iter().enumerate() {
            let (id, label, rank) = group(self, &self.rows[i]);
            groups.entry((rank, label, id)).or_default().push(position);
        }
        let closed = if self.query.text.trim().is_empty() {
            &self.sidebar_groups.closed
        } else {
            &self.sidebar_groups.search_closed
        };
        let mut items = vec![];
        let mut shown = vec![];
        let mut groups: Vec<_> = groups.into_iter().collect();
        if self.section == LibrarySection::Software
            && self
                .table
                .sort()
                .is_some_and(|(_, dir)| dir == SortDir::Descending)
        {
            groups.sort_by(|(a, _), (b, _)| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
        }
        for ((_, label, id), positions) in groups {
            let open = !closed.contains(&id)
                || positions.iter().any(|&v| {
                    self.rows[self.visible[v]]
                        .members
                        .iter()
                        .any(|&i| crate::command::reveals(self, &self.catalog.packages[i]))
                });
            items.push(Item::Header {
                id,
                label,
                count: positions.len(),
                open,
                package: self.rows[self.visible[positions[0]]].package,
            });
            if open {
                for visible in positions {
                    shown.push(self.visible[visible]);
                    items.push(Item::Entry { visible });
                }
            }
        }
        self.sidebar_groups.items = items;
        self.sidebar_groups.shown = shown;
        self.dismiss_hidden_sidebar_overlays();
    }
    pub fn toggle_sidebar_group(&mut self, id: &str) {
        let closed = if self.query.text.trim().is_empty() {
            &mut self.sidebar_groups.closed
        } else {
            &mut self.sidebar_groups.search_closed
        };
        if !closed.remove(id) {
            closed.insert(id.into());
        }
        self.rebuild_sidebar_groups();
        self.sidebar_scroll.reset();
        self.list_revision = self.list_revision.wrapping_add(1);
    }
    fn dismiss_hidden_sidebar_overlays(&mut self) {
        let shown = shown(self);
        let contains = |id: &str| shown.iter().any(|&i| self.rows[i].id == id);
        let close_ai = self
            .ai
            .open
            .as_ref()
            .and_then(|open| open.row.as_deref())
            .is_some_and(|row| !contains(row));
        let close_panel = self
            .sidebar_actions
            .panel
            .as_ref()
            .is_some_and(|p| !contains(&p.row));
        if close_ai {
            self.ai.open = None;
        }
        if close_panel {
            self.sidebar_actions.panel = None;
        }
    }
}

pub fn grouped(s: &State) -> bool {
    s.section != LibrarySection::Software
        || s.table
            .sort()
            .is_some_and(|(column, _)| column == "manager")
}

pub fn shown(s: &State) -> &[usize] {
    if !grouped(s) {
        &s.visible
    } else {
        &s.sidebar_groups.shown
    }
}
pub fn len(s: &State) -> usize {
    if !grouped(s) {
        s.visible.len()
    } else {
        s.sidebar_groups.items.len()
    }
}
pub fn row_index(s: &State, at: usize) -> Option<usize> {
    if !grouped(s) {
        Some(at)
    } else {
        match s.sidebar_groups.items.get(at) {
            Some(Item::Entry { visible }) => Some(*visible),
            _ => None,
        }
    }
}
pub fn item_key(s: &State, at: usize) -> &str {
    if let Some(i) = row_index(s, at) {
        &s.rows[s.visible[i]].id
    } else if let Item::Header { id, .. } = &s.sidebar_groups.items[at] {
        id
    } else {
        unreachable!()
    }
}
pub fn header(s: &State, at: usize, k: &Kit) -> El {
    let Item::Header {
        id,
        label,
        count,
        open,
        package,
    } = &s.sidebar_groups.items[at]
    else {
        unreachable!()
    };
    let id = id.clone();
    let keyboard_id = id.clone();
    let expanded = *open;
    let content = if s.section == LibrarySection::Extensions {
        let p = &s.catalog.packages[*package];
        let editor = p
            .provider
            .map(|m| {
                crate::provider_badge::symbol_sized(m, k, 14.)
                    .key("extension-group-editor")
                    .help(m.editor_name().unwrap_or("Editor"))
            })
            .unwrap_or_else(|| icon(&glyphs::FILE_CODE, 14.).color(k.c.icon_muted));
        row()
            .grow(1.)
            .min_w(0.)
            .gap(6.)
            .center_y()
            .child(editor.a11y_hidden())
            .child(
                text("/")
                    .font(k.ty.text_xsmall)
                    .color(k.c.text_faint)
                    .a11y_hidden(),
            )
            .child(
                crate::profile_icons::view(
                    p.extension.as_ref().and_then(|e| e.profile_icon.as_deref()),
                    k,
                    14.,
                )
                .key("extension-group-profile-icon"),
            )
            .child(
                text(crate::extension_ui::profile_name(p))
                    .key("extension-group-profile-name")
                    .font(k.ty.text_xsmall)
                    .color(k.c.text_muted)
                    .grow(1.)
                    .min_w(0.)
                    .truncate()
                    .help(label)
                    .a11y_hidden(),
            )
    } else {
        text(label)
            .font(k.ty.text_xsmall)
            .color(k.c.text_muted)
            .grow(1.)
            .min_w(0.)
            .truncate()
            .help(label)
    };
    row().role("row").w_full().child(
        col().role("gridcell").w_full().child(
            row()
                .key(format!("{id}-toggle"))
                .role("button")
                .label(format!("{label}, {count} entries"))
                .expanded(*open)
                .focusable(true)
                .w_full()
                .px(7.)
                .py(10.)
                .gap(7.)
                .center_y()
                .radius(4.)
                .hover(k.c.hover)
                .focus_ring(1., 0., k.c.border_focus)
                .on_press_tx(move |s: &mut State, tx| {
                    s.toggle_sidebar_group(&id);
                    tx.instant();
                })
                .on_key(
                    move |s: &mut State, event: &rune::KeyEvent, tx: &mut rune::Tx| {
                        if event.mods.ctrl || event.mods.alt || event.mods.cmd || event.mods.logo {
                            return false;
                        }
                        if (event.key == rune::Key::Left && expanded)
                            || (event.key == rune::Key::Right && !expanded)
                        {
                            s.toggle_sidebar_group(&keyboard_id);
                            tx.instant();
                            true
                        } else {
                            false
                        }
                    },
                )
                .child(content)
                .child(
                    text(count.to_string())
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_faint),
                )
                .child(
                    icon(
                        if *open {
                            &glyphs::CHEVRON_DOWN
                        } else {
                            k.glyph.caret_right
                        },
                        12.,
                    )
                    .color(k.c.text_faint)
                    .a11y_hidden(),
                ),
        ),
    )
}

pub fn provider_scene(s: &mut State) {
    use releasescout_engine::inventory::{self, Manager};
    crate::sidebar_info::description_scene(s);
    s.accept_scan(inventory::parse(Manager::Pnpm, false, &serde_json::json!({
        "root":"C:/fixture/global", "installedExit":0,
        "installed":[{"dependencies":{"provider-fixture":{"version":"1.0.0"}}}],
        "metadata":{"provider-fixture":{"hasCommands":true,"description":"A detected global command-line tool"}}
    }).to_string()).unwrap());
    let row = s.rows.iter_mut().find(|r| r.name == "GitHub CLI").unwrap();
    let mut peer = s.catalog.packages[row.package].clone();
    peer.id = "provider-fixture-winget-peer".into();
    peer.provider = Some(Manager::Winget);
    if let Some(observation) = &mut peer.observation {
        observation.id = "GitHub.cli".into();
    }
    peer.display_priority = 10;
    row.members.push(s.catalog.packages.len());
    s.catalog.packages.push(peer);
    s.table.set_sort("manager", SortDir::Ascending);
    s.sort_visible();
}

pub fn scene(s: &mut State, extensions: bool) {
    if extensions {
        crate::extension_ui::scene(s);
        let ids: Vec<_> = s
            .sidebar_groups
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Header { id, label, .. } if label.ends_with(" / Work") => Some(id.clone()),
                _ => None,
            })
            .collect();
        for id in ids {
            s.toggle_sidebar_group(&id);
        }
    } else {
        crate::watchlist_ui::scene(s);
        s.toggle_sidebar_group("sidebar-group-packages-tooling");
    }
    if let Some(&i) = shown(s).first() {
        let id = s.catalog.packages[s.rows[i].package].id.clone();
        s.select_package(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn software_provider_groups_preserve_memberships_sorting_and_search_state() {
        let mut s = State::new();
        provider_scene(&mut s);
        assert_eq!(
            headers(&s)
                .iter()
                .map(|h| (h.1.as_str(), h.2))
                .collect::<Vec<_>>(),
            [("PNPM", 1), ("Scoop", 2), ("Multiple providers", 1)]
        );
        assert_eq!(shown(&s).len(), s.visible.len());
        assert_eq!(
            shown(&s).iter().collect::<BTreeSet<_>>().len(),
            s.visible.len()
        );
        let gh = s.rows.iter().find(|r| r.name == "GitHub CLI").unwrap();
        assert_eq!(gh.members.len(), 2);
        let gh_id = gh.id.clone();
        s.table.select(gh_id.clone());
        s.toggle_sidebar_group("sidebar-group-software-multiple");
        assert_eq!(shown(&s).len(), 3);
        assert_eq!(s.table.cursor(), Some(gh_id.as_str()));
        s.query = rune::TextEdit::new("GitHub");
        s.refilter();
        assert_eq!(shown(&s).len(), 1);
        s.query = rune::TextEdit::new("");
        s.refilter();
        assert_eq!(shown(&s).len(), 3);
        s.table.set_sort("name", SortDir::Ascending);
        s.sort_visible();
        assert!(!grouped(&s));
        assert_eq!(shown(&s).len(), 4);
        s.table.set_sort("manager", SortDir::Descending);
        s.sort_visible();
        assert_eq!(
            headers(&s).iter().map(|h| h.1.as_str()).collect::<Vec<_>>(),
            ["Scoop", "PNPM", "Multiple providers"]
        );
        assert_eq!(shown(&s).len(), 3);
    }
    fn headers(s: &State) -> Vec<(String, String, usize, bool)> {
        s.sidebar_groups
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Header {
                    id,
                    label,
                    count,
                    open,
                    ..
                } => Some((id.clone(), label.clone(), *count, *open)),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn editor_profile_memberships_are_distinct_and_collapsing_preserves_the_reader_and_data() {
        let mut s = State::new();
        crate::extension_ui::scene(&mut s);
        assert_eq!(headers(&s).len(), 6);
        assert_eq!(s.rows.len(), 9);
        assert_eq!(headers(&s).iter().map(|h| h.2).sum::<usize>(), 9);
        for r in &s.rows {
            let keys: BTreeSet<_> = r
                .members
                .iter()
                .map(|&i| crate::extension_ui::profile_key(&s.catalog.packages[i]))
                .collect();
            assert_eq!(keys.len(), 1);
        }
        let selected = s.selected().unwrap().id.clone();
        let r = s
            .rows
            .iter()
            .find(|r| Some(r.id.as_str()) == s.table.cursor())
            .unwrap();
        let (id, _, _) = group(&s, r);
        let count = headers(&s).iter().find(|h| h.0 == id).unwrap().2;
        let before = s.sidebar_groups.shown.len();
        s.toggle_sidebar_group(&id);
        assert_eq!(s.sidebar_groups.shown.len(), before - count);
        assert_eq!(s.selected().unwrap().id, selected);
        assert_eq!(s.visible.len(), 9);
        s.rebuild_detected();
        assert!(!headers(&s).iter().find(|h| h.0 == id).unwrap().3);
        assert_eq!(s.selected().unwrap().id, selected);
        s.toggle_sidebar_group(&id);
        assert_eq!(s.sidebar_groups.shown.len(), 9);
        assert!(!s.inventory.work.is_pending());
    }
    #[test]
    fn search_opens_matches_without_forgetting_the_normal_collapsed_state() {
        let mut s = State::new();
        crate::watchlist_ui::scene(&mut s);
        assert_eq!(
            headers(&s)
                .iter()
                .map(|h| (h.1.as_str(), h.2))
                .collect::<Vec<_>>(),
            [("Core", 2), ("Tooling", 1)]
        );
        let id = "sidebar-group-packages-core";
        s.toggle_sidebar_group(id);
        assert_eq!(shown(&s).len(), 1);
        s.query = rune::TextEdit::new("next");
        s.refilter();
        assert_eq!(shown(&s).len(), 1);
        assert!(headers(&s)[0].3);
        s.toggle_sidebar_group(id);
        assert!(shown(&s).is_empty());
        s.query = rune::TextEdit::new("");
        s.refilter();
        assert_eq!(shown(&s).len(), 1);
        assert!(!headers(&s)[0].3);
        s.set_section(LibrarySection::Software);
        assert!(s.sidebar_groups.items.is_empty());
        s.set_section(LibrarySection::Packages);
        assert!(!headers(&s)[0].3);
        s.toggle_sidebar_group(id);
        assert_eq!(shown(&s).len(), 3);
    }
}
