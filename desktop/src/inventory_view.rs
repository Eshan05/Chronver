use crate::{
    inventory_workspace::{self as workspace, Column, Record},
    model::State,
    package_info::{copied_field, field, observation_detail, time, value},
    provider_badge,
    rich_select::{self, OptionRow, RichSelect},
    sidebar_filters::{Assessment, UnknownReason, Update},
    sidebar_info::{self, Observation},
    theme,
};
use releasescout_engine::{
    inventory::{InstalledPackage, Manager, Scan},
    version::candidate_order,
};
use rune::{Cx, El, GrowthSpec, Icon, ScrollSpec, TextStyle, col, el, icon, row, text};
use rune_kit::{
    Button, ButtonVariant, Checkbox, IconButton, IconButtonVariant, Kit, SegmentedControl,
    TextInput, glyphs, popover, scroll_bar,
};
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::BTreeSet,
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
    sync::atomic::Ordering,
};

static PACKAGE: Icon = Icon {
    name: "inventory-package",
    view: 24.,
    body: include_str!("../assets/symbols/package.svg"),
};
static REFRESH: Icon = Icon {
    name: "inventory-refresh",
    view: 24.,
    body: include_str!("../assets/status/refresh-cw.svg"),
};
static INFO: Icon = Icon {
    name: "inventory-info",
    view: 24.,
    body: include_str!("../assets/status/info.svg"),
};
static ALERT: Icon = Icon {
    name: "inventory-notice",
    view: 24.,
    body: include_str!("../assets/status/triangle-alert.svg"),
};
static HISTORY: Icon = Icon {
    name: "inventory-unchecked",
    view: 24.,
    body: include_str!("../assets/status/history.svg"),
};
static BAN: Icon = Icon {
    name: "inventory-unsupported",
    view: 24.,
    body: include_str!("../assets/status/ban.svg"),
};

fn label(value: impl Into<String>, k: &Kit) -> El {
    text(value)
        .font(k.ty.text_xsmall)
        .color(k.c.text_muted)
        .min_w(0.)
        .wrap_anywhere()
}

// Raw inventory rows are provider observations, not the sidebar's merged groups.
pub(crate) fn assessment(p: &InstalledPackage, scan: &Scan) -> Assessment {
    let o = Observation::new(p, scan);
    let unknown = |reason| Assessment {
        update: Update::Unknown,
        reason: Some(reason),
    };
    if o.confirmed_update {
        return Assessment {
            update: Update::Available,
            reason: None,
        };
    }
    if !releasescout_engine::inventory_refresh::supports_candidates(p.manager) {
        return unknown(UnknownReason::Unsupported);
    }
    if !o.requested {
        return unknown(UnknownReason::NotChecked);
    }
    if !o.succeeded {
        return unknown(
            if o.detail
                .as_deref()
                .is_some_and(|v| v.starts_with("ambiguous or stale update identity:"))
            {
                UnknownReason::IdentityUnresolved
            } else {
                UnknownReason::CheckFailed
            },
        );
    }
    let Some(from) = p.installed.as_deref().filter(|v| !v.trim().is_empty()) else {
        return unknown(UnknownReason::VersionUnknown);
    };
    if p.available
        .as_deref()
        .is_some_and(|to| candidate_order(from, to).is_none_or(|order| order.is_gt()))
    {
        return unknown(UnknownReason::VersionUnclear);
    }
    Assessment {
        update: Update::Current,
        reason: None,
    }
}

fn status(status: Assessment, k: &Kit) -> El {
    let (glyph, ink, label) = match status.update {
        Update::Available => (&sidebar_info::DOWNLOAD, k.c.success.fg, "Available"),
        Update::Current => (&glyphs::CHECK, k.c.success.fg, "Current"),
        _ => match status.reason.unwrap_or(UnknownReason::NotChecked) {
            UnknownReason::Unsupported => (&BAN, k.c.text_muted, "Unsupported"),
            UnknownReason::CheckFailed => (&glyphs::CIRCLE_X, k.c.danger.fg, "Check failed"),
            UnknownReason::IdentityUnresolved => (&HISTORY, k.c.text_muted, "Unverified"),
            reason => (&HISTORY, k.c.text_muted, reason.label()),
        },
    };
    row()
        .min_w(0.)
        .gap(5.)
        .center_y()
        .help(status.detail())
        .label(label)
        .child(icon(glyph, 13.).color(ink).a11y_hidden())
        .child(
            text(label)
                .font(k.ty.text_xsmall)
                .color(ink)
                .min_w(0.)
                .truncate(),
        )
}

fn provider(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    let state = &s.inventory;
    let manager = Manager::ALL[state.manager];
    RichSelect {
        key: "scan-manager",
        label: "Scan provider",
        current: manager.id(),
        open: state.picker,
        popup_width: 340.,
        value: row()
            .gap(7.)
            .center_y()
            .child(provider_badge::symbol_sized(manager, k, 16.))
            .child(
                text(provider_badge::label(manager))
                    .font(k.ty.ui_compact)
                    .color(k.c.text_base)
                    .min_w(0.)
                    .truncate(),
            ),
        options: Manager::ALL
            .into_iter()
            .map(|m| {
                let scan = state
                    .scans
                    .iter()
                    .find(|scan| scan.manager == m)
                    .or_else(|| state.result.as_ref().filter(|scan| scan.manager == m));
                let detail = scan
                    .map(|scan| {
                        format!(
                            "{} / {}",
                            rich_select::count_label(scan.packages.len(), "observation"),
                            time(&scan.checked_at)
                        )
                    })
                    .unwrap_or_else(|| "Not scanned".into());
                OptionRow {
                    id: m.id().into(),
                    label: provider_badge::label(m).into(),
                    description: detail.clone(),
                    content: row()
                        .gap(10.)
                        .center_y()
                        .child(provider_badge::symbol_sized(m, k, 20.))
                        .child(
                            rich_select::text_content(provider_badge::label(m), &detail, k)
                                .grow(1.),
                        ),
                }
            })
            .collect(),
        on_toggle: |s: &mut State| s.inventory.picker = !s.inventory.picker,
        on_close: |s: &mut State| s.inventory.picker = false,
        on_choose: |s: &mut State, id, tx| {
            if let Some(index) = Manager::ALL.iter().position(|m| m.id() == id) {
                s.inventory.manager = index;
                s.reset_inventory_rows(tx);
            }
        },
    }
    .build_compact(k, layers)
    .w(168.)
}

fn table_row() -> El {
    row().w_full().min_w(0.).gap(8.).center_y()
}

fn table_header(s: &State, rows: usize, selected: usize, k: &Kit) -> El {
    let mut header = table_row()
        .key("inventory-table-header")
        .py(7.)
        .fill(k.c.layer_01)
        .sticky_top(0.)
        .border_sides([0., 0., 1., 0.], theme::separator(k))
        .child(
            Checkbox::new(rows > 0 && selected == rows)
                .indeterminate(selected > 0 && selected < rows)
                .label("Select all matching observations")
                .hide_label()
                .on_toggle_tx(|s: &mut State, tx| {
                    let keys: Vec<_> = workspace::filtered(s).into_iter().map(|r| r.key).collect();
                    let all = keys
                        .iter()
                        .all(|key| s.inventory.workspace.selected.contains(key));
                    for key in keys {
                        if all {
                            s.inventory.workspace.selected.remove(&key);
                        } else {
                            s.inventory.workspace.selected.insert(key);
                        }
                    }
                    tx.instant();
                })
                .build(k)
                .w(24.)
                .key("inventory-select-all"),
        );
    for column in std::iter::once(Column::Name).chain(
        Column::OPTIONAL
            .into_iter()
            .filter(|c| s.inventory.workspace.columns.contains(c)),
    ) {
        let mut cell = row()
            .gap(4.)
            .center_y()
            .role("button")
            .label(format!("Sort by {}", column.label()))
            .help(format!("Sort by {}", column.label()))
            .focusable(true)
            .focus_ring(1., 0., k.c.border_focus)
            .on_press_tx(move |s: &mut State, tx| s.sort_inventory(column, tx))
            .child(label(column.label(), k).truncate());
        if s.inventory.workspace.sort == column {
            cell = cell.child(
                icon(
                    if s.inventory.workspace.descending {
                        &glyphs::CHEVRON_DOWN
                    } else {
                        &glyphs::CHEVRON_UP
                    },
                    12.,
                )
                .color(k.c.text_muted),
            );
        }
        cell = if column == Column::Name {
            cell.grow(1.).min_w(160.).pl(30.)
        } else {
            cell.w(column.width())
        };
        header = header.child(cell.key(format!("inventory-sort-{column:?}")));
    }
    header.child(el().w(52.))
}
fn version(raw: Option<&str>, k: &Kit) -> El {
    text(raw.unwrap_or("-"))
        .font(k.ty.text_xsmall.mono())
        .color(if raw.is_some() {
            k.c.text_base
        } else {
            k.c.text_muted
        })
        .min_w(0.)
        .truncate()
        .help(raw.unwrap_or("Version not reported"))
}

fn details(p: &InstalledPackage, scan: &Scan, k: &Kit) -> El {
    let o = Observation::new(p, scan);
    let mut body = col()
        .key("inventory-record-details")
        .w_full()
        .gap(0.)
        .pl(18.)
        .py(6.)
        .child(copied_field("inventory-record-id", "Package ID", &p.id, k))
        .child(field(
            "inventory-record-source",
            "Provider source",
            value(&p.source, k, false),
            k,
        ))
        .child(field(
            "inventory-record-scope",
            "Scope",
            value(&p.scope, k, false),
            k,
        ))
        .child(field(
            "inventory-record-observed",
            "Last scan",
            value(time(&scan.checked_at), k, false),
            k,
        ));
    if let Some(id) = p
        .evidence
        .registration_id
        .as_deref()
        .filter(|id| *id != p.id)
    {
        body = body.child(copied_field(
            "inventory-record-registration",
            "Registration",
            id,
            k,
        ));
    }
    if let Some(root) = &p.root {
        body = body.child(copied_field(
            "inventory-record-location",
            "Location",
            root,
            k,
        ));
    }
    if p.pinned {
        body = body.child(field(
            "inventory-record-pinned",
            "Pinned",
            value("Yes", k, false),
            k,
        ));
    }
    if let Some(detail) = observation_detail(&o) {
        body = body.child(value(detail, k, false).pl(18.));
    }
    body
}

fn mark(s: &State, r: &Record<'_>, k: &Kit) -> El {
    let name = &r.name;
    r.linked
        .map(|p| {
            crate::ui::mark(
                p,
                k,
                if s.inventory.workspace.compact {
                    22.
                } else {
                    26.
                },
            )
        })
        .unwrap_or_else(|| {
            el().size(
                if s.inventory.workspace.compact {
                    22.
                } else {
                    26.
                },
                if s.inventory.workspace.compact {
                    22.
                } else {
                    26.
                },
            )
            .radius(4.)
            .fill(k.c.layer_02)
            .center()
            .child(
                text(name.chars().take(2).collect::<String>().to_uppercase())
                    .font(k.ty.text_xsmall)
                    .color(k.c.text_muted),
            )
        })
}

fn group(
    s: &State,
    group: &workspace::Group,
    first: &Record<'_>,
    selected: usize,
    k: &Kit,
) -> El {
    let open = !s.inventory.workspace.collapsed_groups.contains(&group.key);
    let selection_key = group.key.clone();
    let toggle_key = group.key.clone();
    let header = table_row()
        .key(format!("inventory-group-{}", group.key))
        .py(5.)
        .border_sides([0., 0., 1., 0.], theme::separator(k))
        .child(
            Checkbox::new(selected == group.members.len())
                .indeterminate(selected > 0 && selected < group.members.len())
                .label(format!(
                    "Select all matching observations of {}",
                    first.name
                ))
                .hide_label()
                .on_toggle_tx(move |s: &mut State, tx| {
                    s.toggle_inventory_group_selection(&selection_key, tx);
                })
                .build(k)
                .w(24.)
                .key(format!("inventory-group-select-{}", group.key)),
        )
        .child(
            row()
                .grow(1.)
                .min_w(160.)
                .gap(8.)
                .center_y()
                .role("button")
                .focusable(true)
                .expanded(open)
                .focus_ring(1., 0., k.c.border_focus)
                .label(format!(
                    "{}: {} observations",
                    first.name,
                    group.members.len()
                ))
                .on_press_tx(move |s: &mut State, tx| {
                    if !s.inventory.workspace.collapsed_groups.remove(&toggle_key) {
                        s.inventory
                            .workspace
                            .collapsed_groups
                            .insert(toggle_key.clone());
                    }
                    tx.instant();
                })
                .key(format!("inventory-group-toggle-{}", group.key))
                .child(
                    icon(
                        if open {
                            &glyphs::CHEVRON_DOWN
                        } else {
                            k.glyph.caret_right
                        },
                        14.,
                    )
                    .color(k.c.text_muted),
                )
                .child(mark(s, first, k))
                .child(
                    text(&first.name)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .min_w(0.)
                        .truncate(),
                )
                .child(
                    text(group.members.len().to_string())
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_muted),
                ),
        );
    crate::inventory_menu::trigger(crate::inventory_menu::Target::Group(group.key.clone()), header, k)
}

fn range_destination(s: &State) -> Option<&str> {
    let selected = s.selected()?;
    let profile = s.live.catalog.packages.iter().find(|p| p.id == selected.id)?;
    let component = s.live.sessions.get(&selected.id).map_or(0, |q| q.component);
    let stream = profile.streams.get(component)?;
    (stream.basis == releasescout_engine::Basis::Version).then_some(&selected.name)
}

fn record(
    s: &State,
    r: &Record<'_>,
    k: &Kit,
    child: bool,
    destination: Option<&str>,
) -> El {
    let p = r.package;
    let scan = r.scan;
    let name = &r.name;
    let expanded = s.inventory.details.as_deref() == Some(&r.key);
    let selected = s.inventory.workspace.selected.contains(&r.key);
    let select_key = r.key.clone();
    let child_label = if child {
        r.linked
            .filter(|p| p.extension.is_some())
            .map(|p| crate::extension_ui::profile_name(p).to_owned())
            .unwrap_or_else(|| {
                let parts: Vec<_> = [&p.scope, &p.source]
                    .into_iter()
                    .filter(|v| !v.trim().is_empty() && *v != "unknown")
                    .cloned()
                    .collect();
                if parts.is_empty() {
                    p.id.clone()
                } else {
                    parts.join(" / ")
                }
            })
    } else {
        name.clone()
    };
    let mut identity = col().grow(1.).min_w(0.).gap(2.).child(
        text(&child_label)
            .font(k.ty.ui_medium)
            .color(k.c.text_base)
            .min_w(0.)
            .truncate()
            .help(if child {
                format!(
                    "{} / {} / {}",
                    name,
                    provider_badge::label(p.manager),
                    child_label
                )
            } else {
                name.clone()
            }),
    );
    if !s.inventory.workspace.compact {
        identity = identity.child(label(format!("{} / {}", p.id, p.scope), k).truncate());
    }
    let mut line = table_row()
        .key(format!("inventory-row-{}", r.key))
        .py(if s.inventory.workspace.compact {
            4.
        } else {
            8.
        })
        .fill(if selected {
            k.c.layer_02
        } else {
            rune::Color::TRANSPARENT
        })
        .hover(k.c.hover)
        .child(
            Checkbox::new(selected)
                .label(format!(
                    "Select {name} from {}",
                    provider_badge::label(p.manager)
                ))
                .hide_label()
                .on_toggle_tx(move |s: &mut State, tx| {
                    if workspace::records(s).iter().any(|r| r.key == select_key)
                        && !s.inventory.workspace.selected.remove(&select_key)
                    {
                        s.inventory.workspace.selected.insert(select_key.clone());
                    }
                    tx.instant();
                })
                .build(k)
                .w(24.)
                .key(format!("inventory-select-{}", r.key)),
        )
        .child(
            row()
                .grow(1.)
                .min_w(160.)
                .gap(8.)
                .center_y()
                .children(child.then(|| el().w(14.)))
                .child(if child {
                    provider_badge::symbol_sized(p.manager, k, 16.)
                } else {
                    mark(s, r, k)
                })
                .child(identity),
        );
    for column in Column::OPTIONAL
        .into_iter()
        .filter(|c| s.inventory.workspace.columns.contains(c))
    {
        let cell = match column {
            Column::Id => version(Some(&p.id), k),
            Column::Installed => version(p.installed.as_deref(), k),
            Column::Candidate => {
                let mut cell = version(p.available.as_deref(), k);
                if assessment(p, scan).update == Update::Available
                    && let Some(size) = p
                        .installed
                        .as_deref()
                        .zip(p.available.as_deref())
                        .and_then(|(from, to)| sidebar_info::UpdateSize::between(from, to))
                {
                    cell = cell.color(size.color(s.dark));
                }
                cell
            }
            Column::Status => status(assessment(p, scan), k),
            Column::Source => row()
                .gap(5.)
                .center_y()
                .min_w(0.)
                .child(provider_badge::badge(p.manager, k))
                .child(label(&p.source, k).truncate())
                .help(format!(
                    "{} / {}",
                    provider_badge::label(p.manager),
                    p.source
                )),
            Column::Scope => label(&p.scope, k).truncate().help(&p.scope),
            Column::Name => unreachable!(),
        };
        line = line.child(cell.w(column.width()).shrink());
    }
    let check_key = r.key.clone();
    let range_key = r.key.clone();
    line = line.child(
        row()
            .w(52.)
            .gap(4.)
            .child(
                IconButton::new(&INFO)
                    .variant(IconButtonVariant::Ghost)
                    .label(format!("Details for {name}"))
                    .help("Observation details")
                    .expanded(expanded)
                    .on_press_tx(move |s: &mut State, tx| {
                        s.inventory.details = if s.inventory.details.as_deref() == Some(&check_key)
                        {
                            None
                        } else {
                            Some(check_key.clone())
                        };
                        tx.instant();
                    })
                    .build(k)
                    .size(24., 24.)
                    .key(format!("inventory-details-{}", r.key)),
            )
            .child(
                IconButton::new(&glyphs::ARROW_RIGHT)
                    .variant(IconButtonVariant::Ghost)
                    .label(format!("Use versions from {name}"))
                    .help(if let Some(destination) = destination {
                        format!("Use this range for {destination}; verify the product match")
                    } else {
                        "Select a version-based source in Notes first".into()
                    })
                    .disabled(destination.is_none() || p.installed.is_none() || p.available.is_none())
                    .on_press_tx(move |s: &mut State, tx| {
                        s.use_inventory_key(&range_key);
                        tx.instant();
                    })
                    .build(k)
                    .size(24., 24.)
                    .key(format!("use-range-{}", r.key)),
            ),
    );
    let mut result = col()
        .w_full()
        .min_w(0.)
        .border_sides([0., 0., 1., 0.], theme::separator(k))
        .child(line)
        .children(expanded.then(|| details(p, scan, k)));
    if child {
        result = result.child(
            el().key("inventory-branch-guide")
                .absolute(39., 0.)
                .w(1.)
                .h_full()
                .fill(theme::separator(k))
                .a11y_hidden(),
        );
    }
    crate::inventory_menu::trigger(
        crate::inventory_menu::Target::Observation(r.key.clone()),
        result,
        k,
    )
}

struct RecordRef {
    manager: Manager,
    index: usize,
    key: String,
    name: String,
    linked_index: Option<usize>,
}

impl From<Record<'_>> for RecordRef {
    fn from(r: Record<'_>) -> Self {
        Self {
            manager: r.scan.manager,
            index: r.index,
            key: r.key,
            name: r.name,
            linked_index: r.linked_index,
        }
    }
}

impl RecordRef {
    fn resolve<'a>(&self, s: &'a State) -> Option<Record<'a>> {
        let scan = s
            .inventory
            .scans
            .iter()
            .find(|scan| scan.manager == self.manager)
            .or_else(|| {
                s.inventory
                    .result
                    .as_ref()
                    .filter(|scan| scan.manager == self.manager)
            })?;
        Some(Record {
            scan,
            package: scan.packages.get(self.index)?,
            index: self.index,
            key: self.key.clone(),
            name: self.name.clone(),
            linked: self
                .linked_index
                .and_then(|index| s.catalog.packages.get(index)),
            linked_index: self.linked_index,
        })
    }
}

#[derive(Hash)]
enum TableRow {
    Group(usize),
    Observation(usize, bool),
    End,
}

struct Projection {
    groups: Vec<workspace::Group>,
    records: Vec<RecordRef>,
    entries: Vec<(String, TableRow)>,
    row_version: u64,
}

struct ProjectionKey {
    list_revision: u64,
    query: String,
    status: Update,
    sort: Column,
    descending: bool,
    hidden_providers: BTreeSet<String>,
    hidden_sources: BTreeSet<(String, String)>,
    collapsed_groups: BTreeSet<String>,
}

#[derive(Default)]
pub struct ProjectionCache(RefCell<Option<(ProjectionKey, Rc<Projection>)>>);

impl ProjectionCache {
    fn get(&self, s: &State) -> Rc<Projection> {
        let w = &s.inventory.workspace;
        // Real scans are applied through reconciliation, which advances list_revision.
        // A standalone scan result can be replaced without reconciliation.
        if s.inventory.result.as_ref().is_some_and(|scan| {
            !s.inventory.scans.iter().any(|other| other.manager == scan.manager)
        }) {
            return Rc::new(project_rows(s));
        }
        let query = s.inventory.filter.text.trim();
        let mut cache = self.0.borrow_mut();
        if let Some((key, projection)) = &*cache
            && key.list_revision == s.list_revision
            && key.query == query
            && key.status == s.inventory.status_filter
            && key.sort == w.sort
            && key.descending == w.descending
            && key.hidden_providers == w.hidden_providers
            && key.hidden_sources == w.hidden_sources
            && key.collapsed_groups == w.collapsed_groups
        {
            return projection.clone();
        }
        let projection = Rc::new(project_rows(s));
        *cache = Some((
            ProjectionKey {
                list_revision: s.list_revision,
                query: query.to_owned(),
                status: s.inventory.status_filter,
                sort: w.sort,
                descending: w.descending,
                hidden_providers: w.hidden_providers.clone(),
                hidden_sources: w.hidden_sources.clone(),
                collapsed_groups: w.collapsed_groups.clone(),
            },
            projection.clone(),
        ));
        projection
    }
}

fn project_rows(s: &State) -> Projection {
    let rows = workspace::filtered(s);
    let groups = workspace::groups(&rows);
    let mut entries = Vec::new();
    let mut keys = std::collections::HashSet::new();
    for (index, bucket) in groups.iter().enumerate() {
        let child = bucket.members.len() > 1;
        if child {
            entries.push((
                format!("inventory-item-group-{}", bucket.key),
                TableRow::Group(index),
            ));
            if s.inventory.workspace.collapsed_groups.contains(&bucket.key) {
                continue;
            }
        }
        for &index in &bucket.members {
            let key = &rows[index].key;
            let unique_key = if keys.insert(key) {
                format!("inventory-item-{key}")
            } else {
                format!("inventory-item-{key}-{}", rows[index].index)
            };
            entries.push((unique_key, TableRow::Observation(index, child)));
        }
    }
    entries.push(("inventory-scroll-end".into(), TableRow::End));

    let mut hasher = DefaultHasher::new();
    entries.hash(&mut hasher);
    Projection {
        groups,
        records: rows.into_iter().map(RecordRef::from).collect(),
        entries,
        row_version: hasher.finish(),
    }
}

fn virtual_table(s: &State, projection: Rc<Projection>, k: &Kit, width: f32) -> El {
    let row_keys = projection.clone();
    let row_count = projection.entries.len();
    let row_version = projection.row_version;
    let kit = k.clone();
    let destination = range_destination(s).map(str::to_owned);
    col()
        .key("inventory-rows")
        .role("list")
        .label("Inventory observations")
        .w_full()
        .min_w(width)
        .px(16.)
        .growth(GrowthSpec::default())
        .virtual_rows(
            row_count,
            if s.inventory.workspace.compact {
                32.
            } else {
                50.
            },
            move |_: &State, index| Cow::Owned(row_keys.entries[index].0.clone()),
            move |s: &State, index| match projection.entries[index].1 {
                TableRow::Group(index) => {
                    let bucket = &projection.groups[index];
                    let Some(first) = projection.records[bucket.members[0]].resolve(s) else {
                        return el();
                    };
                    let selected = bucket
                        .members
                        .iter()
                        .filter(|&&i| {
                            s.inventory.workspace.selected.contains(&projection.records[i].key)
                        })
                        .count();
                    col()
                        .w_full()
                        .role("listitem")
                        .child(group(s, bucket, &first, selected, &kit))
                }
                TableRow::Observation(index, child) => {
                    projection.records[index].resolve(s).map_or_else(el, |r| {
                        col().w_full().role("listitem").child(record(
                            s,
                            &r,
                            &kit,
                            child,
                            destination.as_deref(),
                        ))
                    })
                }
                TableRow::End => el()
                    .w_full()
                    .h(crate::app_chrome::DOCK_CLEARANCE)
                    .a11y_hidden(),
            },
        )
        .virtual_rows_version(row_version)
}

fn empty(title: &str, glyph: &'static Icon, k: &Kit) -> El {
    col()
        .key("inventory-empty")
        .w_full()
        .py(48.)
        .gap(10.)
        .center()
        .child(icon(glyph, 24.).color(k.c.text_muted))
        .child(
            text(title)
                .font(k.ty.ui_medium)
                .color(k.c.text_muted)
                .wrap(),
        )
}

fn notice_section(title: &str, messages: Vec<String>, attention: bool, k: &Kit) -> El {
    let glyph = if attention { &glyphs::CIRCLE_X } else { &ALERT };
    let ink = if attention {
        k.c.danger.fg
    } else {
        k.c.text_muted
    };
    col()
        .w_full()
        .gap(7.)
        .px(12.)
        .py(9.)
        .child(
            row()
                .w_full()
                .gap(7.)
                .center_y()
                .child(icon(glyph, 14.).color(ink).a11y_hidden())
                .child(
                    text(title)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .grow(1.),
                )
                .child(label(messages.len().to_string(), k)),
        )
        .child(
            col()
                .w_full()
                .pl(21.)
                .gap(5.)
                .children(messages.into_iter().map(|message| {
                    let provider = message.split_once(": ").and_then(|(prefix, detail)| {
                        Manager::ALL
                            .into_iter()
                            .find(|m| {
                                prefix.eq_ignore_ascii_case(m.id())
                                    || prefix.eq_ignore_ascii_case(provider_badge::label(*m))
                            })
                            .map(|m| (m, detail))
                    });
                    if let Some((manager, detail)) = provider {
                        row()
                            .w_full()
                            .gap(7.)
                            .child(row().w(64.).child(provider_badge::badge(manager, k)))
                            .child(label(detail, k).grow(1.).selectable(true))
                    } else {
                        label(message, k).selectable(true)
                    }
                })),
        )
}

pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>, cx: &Cx<State>) -> (El, (usize, usize)) {
    let state = &s.inventory;
    let manager = Manager::ALL[state.manager];
    let scans = workspace::scans(s);
    let total = scans.iter().map(|scan| scan.packages.len()).sum::<usize>();
    let supported = releasescout_engine::inventory_refresh::supports_candidates(manager);
    let busy = state.loading || state.discovery.iter().any(|work| work.is_pending());
    let notices = crate::app_chrome::notice_messages(s);
    let mut warnings = Vec::new();
    for scan in &scans {
        for warning in &scan.warnings {
            let warning = format!("{}: {warning}", provider_badge::label(scan.manager));
            if !notices.contains(&warning) && !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
    }
    let mut header = col()
        .w_full()
        .gap(14.)
        .px(16.)
        .pt(18.)
        .pb(12.)
        .child(
            row()
                .w_full()
                .gap(8.)
                .center_y()
                .child(icon(&PACKAGE, 19.).color(k.c.text_muted))
                .child(
                    text("Inventory")
                        .font(TextStyle::new(17., 600.).lh(24.))
                        .color(k.c.text_base)
                        .grow(1.),
                )
                .child(
                    Button::new("Rescan all")
                        .variant(ButtonVariant::Ghost)
                        .icon(&REFRESH)
                        .disabled(busy)
                        .on_press(|s: &mut State| s.start_discovery())
                        .build(k)
                        .key("discover-all"),
                )
                .child(
                    IconButton::new(&glyphs::FILE_TEXT)
                        .variant(IconButtonVariant::Ghost)
                        .label("Back to notes")
                        .help("Back to notes")
                        .on_press_tx(|s: &mut State, tx| {
                            s.inventory.workspace.context = None;
                            s.inventory.open = false;
                            s.mark_current_notes_read();
                            tx.instant();
                        })
                        .build(k)
                        .size(28., 28.)
                        .key("inventory-back"),
                ),
        )
        .child(
            row()
                .w_full()
                .gap(8.)
                .center_y()
                .flex_wrap()
                .child(provider(s, k, layers))
                .child(
                    Button::new("Scan installed")
                        .icon(&REFRESH)
                        .disabled(state.loading)
                        .on_press(|s: &mut State| s.start_inventory(false))
                        .build(k)
                        .h(28.)
                        .key("scan-installed"),
                )
                .child(
                    Button::new("Check candidates")
                        .icon(&sidebar_info::DOWNLOAD)
                        .disabled(state.loading || !supported)
                        .help(if supported {
                            "Check this provider for newer versions"
                        } else {
                            "This provider does not check update candidates"
                        })
                        .on_press(|s: &mut State| s.start_inventory(true))
                        .build(k)
                        .h(28.)
                        .key("scan-updates"),
                )
                .children(state.loading.then(|| {
                    IconButton::new(&glyphs::CLOSE)
                        .variant(IconButtonVariant::Ghost)
                        .label("Cancel scan")
                        .help("Cancel scan")
                        .on_press(|s: &mut State| {
                            s.inventory.cancel.store(true, Ordering::Relaxed);
                            s.inventory.work.cancel();
                            s.inventory.loading = false;
                            s.inventory.error = Some("Scan canceled".into());
                        })
                        .build(k)
                        .size(28., 28.)
                        .key("cancel-scan")
                })),
        );
    let mut summary = row().w_full().gap(8.).center_y().flex_wrap();
    if state.loading {
        let active = state.last_request.map_or(manager, |(m, _)| m);
        summary = summary
            .child(icon(&REFRESH, 12.).color(k.c.text_muted))
            .child(
                label(format!("Checking {}...", provider_badge::label(active)), k)
                    .key("scan-loading"),
            );
    } else if !scans.is_empty() {
        summary = summary.child(label(
            format!("{} providers / {total} observations", scans.len()),
            k,
        ));
    } else {
        summary = summary.child(label("No saved scan", k));
    }
    summary = summary.child(el().grow(1.));
    let has_details = !notices.is_empty() || !warnings.is_empty();
    if has_details {
        summary = summary.child(
            Button::new(if notices.is_empty() {
                "Provider warnings".into()
            } else {
                crate::rich_select::count_label(notices.len(), "notice")
            })
            .variant(ButtonVariant::Ghost)
            .icon(&ALERT)
            .expanded(state.notices_open)
            .on_press(|s: &mut State| s.inventory.notices_open = !s.inventory.notices_open)
            .build(k)
            .h(24.)
            .key("inventory-notices"),
        );
    }
    header = header.child(summary);
    if let Some(error) = &state.error
        && !state.notices_open
    {
        header = header.child(
            row()
                .w_full()
                .gap(6.)
                .center_y()
                .child(icon(&glyphs::CIRCLE_X, 13.).color(k.c.danger.fg))
                .child(label(error, k).max_lines(1))
                .key("scan-error"),
        );
    }
    if state.notices_open && has_details {
        let divider = !notices.is_empty() && !warnings.is_empty();
        header = header.child(
            scroll_bar(
                k,
                "inventory-notice-area",
                "scroll",
                popover::fit_scroll(
                    col()
                        .w_full()
                        .children(
                            (!notices.is_empty())
                                .then(|| notice_section("Needs attention", notices, true, k)),
                        )
                        .children(
                            divider.then(|| {
                                el().w_full().h(1.).fill(theme::separator(k)).a11y_hidden()
                            }),
                        )
                        .children(
                            (!warnings.is_empty())
                                .then(|| notice_section("Provider warnings", warnings, false, k)),
                        ),
                ),
            )
            .w_full()
            .max_h(152.)
            .min_h(0.)
            .radius(6.)
            .fill(k.c.layer_01)
            .border(1., theme::separator(k))
            .key("inventory-notice-details"),
        );
    }
    let projection = state.projection.get(s);
    let matched_count = projection.records.len();
    let visible_selected = if state.workspace.selected.is_empty() {
        0
    } else {
        projection
            .records
            .iter()
            .filter(|r| state.workspace.selected.contains(&r.key))
            .count()
    };
    header = header
        .child(
            row()
                .w_full()
                .gap(10.)
                .center_y()
                .flex_wrap()
                .child(
                    TextInput::new(&state.filter)
                        .label("Filter detected software")
                        .placeholder("Search name, ID or source...")
                        .leading_icon(&glyphs::SEARCH)
                        .clear_button()
                        .on_edit(|s: &mut State| &mut s.inventory.filter)
                        .on_change(|s: &mut State, tx| s.reset_inventory_rows(tx))
                        .build(k)
                        .grow(1.)
                        .min_w(160.)
                        .key("scan-filter"),
                )
                .child(
                    SegmentedControl::new(
                        &["All", "Updates", "Current", "Unknown"],
                        Some(state.status_filter as usize),
                    )
                    .width(Some(340.))
                    .label("Inventory update status")
                    .on_pick(|s: &mut State, index, tx| {
                        s.inventory.status_filter = [
                            Update::Any,
                            Update::Available,
                            Update::Current,
                            Update::Unknown,
                        ][index];
                        s.reset_inventory_rows(tx);
                    })
                    .build(k)
                    .key("inventory-status-filter"),
                ),
        )
        .child(crate::inventory_tools::bar(s, matched_count, visible_selected, k, layers));
    let min_width = 24.
        + 160.
        + 52.
        + 32.
        + (state.workspace.columns.len() + 2) as f32 * 8.
        + state
            .workspace
            .columns
            .iter()
            .map(|c| c.width())
            .sum::<f32>();
    let width = cx
        .layout_with("inventory-scroll-area", |r| r[2])
        .unwrap_or(0.)
        .max(min_width);
    let column_header = table_header(s, matched_count, visible_selected, k).min_w(width).px(16.);
    crate::inventory_menu::view(s, k, layers);
    crate::inventory_tools::dialogs(s, k, layers);
    let body = if matched_count == 0 {
        empty(
            if scans.is_empty() {
                "No saved inventory"
            } else {
                "No matching observations"
            },
            &PACKAGE,
            k,
        )
    } else {
        virtual_table(s, projection, k, width)
    };
    let view = col()
        .w_full()
        .h_full()
        .min_w(0.)
        .min_h(0.)
        .fill(k.c.bg_base)
        .child(header)
        .child(
            scroll_bar(
                k,
                "inventory-scroll-area",
                "scroll",
                col()
                    .key("detected-inventory")
                    .w_full()
                    .grow(1.0)
                    .min_w(0.)
                    .min_h(0.)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .scroll_xy()
                    .child(column_header)
                    .child(body),
            )
            .w_full()
            .grow(1.)
            .min_h(0.),
        );
    (view, (matched_count, total))
}

pub fn scene(s: &mut State) {
    let mut scan=releasescout_engine::inventory::parse(Manager::Scoop,true,&serde_json::json!({
        "installed":{"apps":[
            {"Name":"gh","Version":"2.0.0","Source":"main"},
            {"Name":"pnpm","Version":"10.0.0","Source":"main"},
            {"Name":"fzf","Version":"0.74.0","Source":"main"},
            {"Name":"nodejs","Version":"22.0.0","Source":"main"},
            {"Name":"ripgrep","Version":"15.0.0","Source":"main"},
            {"Name":"fixture-broken","Version":"1.0.0","Source":"extras"},
            {"Name":"fixture-unknown","Source":"extras"}
        ]},
        "metadata":{"gh":{"root":"C:/fixture/scoop/apps/gh/current","description":"GitHub's command-line interface.","hasCommands":true}},
        "updates":[
            {"Name":"gh","Installed Version":"2.0.0","Latest Version":"2.1.0"},
            {"Name":"fzf","Installed Version":"0.74.0","Latest Version":"0.75.0"},
            {"Name":"nodejs","Installed Version":"22.0.0","Latest Version":"24.0.0"},
            {"Name":"fixture-broken","Installed Version":"0.9.0","Latest Version":"2.0.0"}
        ]
    }).to_string()).unwrap();
    for p in &mut scan.packages {
        if p.id != "fixture-broken" && p.installed.is_some() {
            p.candidate_check_succeeded = Some(true);
        }
    }
    s.accept_scan(scan);
    s.inventory.manager = Manager::ALL
        .iter()
        .position(|m| *m == Manager::Scoop)
        .unwrap();
    s.inventory.open = true;
    let id = s
        .catalog
        .packages
        .iter()
        .find(|p| p.observation.as_ref().is_some_and(|o| o.id == "gh"))
        .unwrap()
        .id
        .clone();
    s.select_package(&id);
}

pub fn workspace_scene(s: &mut State) {
    scene(s);
    for (manager, source, scope, items) in [
        (
            Manager::Winget,
            "winget",
            "user",
            vec![
                ("GitHub.cli", "GitHub CLI", "2.0.0", Some("2.1.0")),
                ("Figma.Figma", "Figma", "125.0.0", None),
            ],
        ),
        (
            Manager::Npm,
            "npm",
            "global",
            vec![
                ("@fixture/formatter", "Formatter", "1.9.0", Some("1.10.0")),
                ("fixture-tool", "Fixture Tool", "1.0.0", None),
            ],
        ),
        (
            Manager::Local,
            "Windows uninstall registration",
            "user",
            vec![(
                "ARP\\User\\X64\\fixture",
                "Standalone Desktop",
                "3.0.0",
                None,
            )],
        ),
    ] {
        let packages:Vec<_>=items.into_iter().map(|(id,name,installed,available)|serde_json::json!({
            "manager":manager,"id":id,"name":name,"installed":installed,"available":available,
            "candidate_check_succeeded":true,"source":source,"scope":scope,"root":null,"pinned":false,"install_origin_known":false
        })).collect();
        let scan=serde_json::from_value(serde_json::json!({"manager":manager,"checked_at":"2026-11-15T01:00:00Z",
            "update_check_requested":manager!=Manager::Local,"update_check_succeeded":manager!=Manager::Local,"warnings":[],"packages":packages})).unwrap();
        s.accept_scan(scan);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_projection_reuses_rows_but_tracks_filters_and_scan_changes() {
        let mut s = State::new();
        scene(&mut s);
        let first = s.inventory.projection.get(&s);
        s.inventory.workspace.selected.insert(first.records[0].key.clone());
        assert!(Rc::ptr_eq(&first, &s.inventory.projection.get(&s)));

        s.inventory.filter = rune::TextEdit::new("fixture-unknown");
        let filtered = s.inventory.projection.get(&s);
        assert_eq!(filtered.records.len(), 1);
        assert!(!Rc::ptr_eq(&first, &filtered));
        s.inventory.workspace.hidden_providers.insert(Manager::Scoop.id().into());
        assert!(s.inventory.projection.get(&s).records.is_empty());

        s.inventory.workspace.hidden_providers.clear();
        let before_scan = s.inventory.projection.get(&s);
        let mut scan = s.inventory.selected_scan().unwrap().clone();
        scan.packages.retain(|p| p.id != "fixture-unknown");
        s.accept_scan(scan);
        let after_scan = s.inventory.projection.get(&s);
        assert!(!Rc::ptr_eq(&before_scan, &after_scan));
        assert!(after_scan.records.is_empty());
    }

    #[test]
    fn raw_status_requires_a_successful_check_and_comparable_versions() {
        let mut s = State::new();
        scene(&mut s);
        let scan = s.inventory.selected_scan().unwrap();
        for (id, expected) in [
            ("gh", Update::Available),
            ("pnpm", Update::Current),
            ("fixture-broken", Update::Unknown),
            ("fixture-unknown", Update::Unknown),
        ] {
            assert_eq!(
                assessment(scan.packages.iter().find(|p| p.id == id).unwrap(), scan).update,
                expected,
                "{id}"
            );
        }
        let mut scan = scan.clone();
        let mut p = scan
            .packages
            .iter()
            .find(|p| p.id == "pnpm")
            .unwrap()
            .clone();
        scan.update_check_requested = false;
        assert_eq!(
            assessment(&p, &scan).reason,
            Some(UnknownReason::NotChecked)
        );
        scan.update_check_requested = true;
        p.candidate_check_succeeded = Some(false);
        assert_eq!(assessment(&p, &scan).update, Update::Unknown);
        p.candidate_check_succeeded = Some(true);
        p.available = Some("not-a-version".into());
        assert_eq!(
            assessment(&p, &scan).reason,
            Some(UnknownReason::VersionUnclear)
        );
        p.manager = Manager::Local;
        assert_eq!(
            assessment(&p, &scan).reason,
            Some(UnknownReason::Unsupported)
        );
    }
}
