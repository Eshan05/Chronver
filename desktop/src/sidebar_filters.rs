use crate::model::{LibrarySection, Package, PackageRow, State};
use releasescout_engine::{inventory::Manager, version::candidate_order};
use rune::{El, Icon, TextStyle, Tx, col, el, icon, row, text};
use rune_kit::{Button, Checkbox, Kit, Placement, SegmentedControl, popover, scrim, scroll_bar};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Update {
    #[default]
    Any,
    Available,
    Current,
    Unknown,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Facets {
    pub update: Update,
    pub source: Option<bool>,
    pub notes: Option<bool>,
    pub providers: BTreeSet<String>,
    pub include_support: bool,
}

impl Facets {
    pub fn valid(&self) -> bool {
        self.providers.len() <= Manager::ALL.len() + 1
            && self
                .providers
                .iter()
                .all(|id| id == "unrecorded" || Manager::ALL.iter().any(|m| m.id() == id))
    }
}

pub fn provider(p: &Package) -> Option<Manager> {
    p.provider.or_else(|| {
        (p.watchlist
            .as_ref()
            .is_some_and(|entry| entry.ecosystem == releasescout_engine::watchlist::Ecosystem::Npm)
            || p.dependency.is_some())
        .then_some(Manager::Npm)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnknownReason {
    Unsupported,
    NotChecked,
    VersionUnknown,
    VersionUnclear,
    CheckFailed,
    IdentityUnresolved,
}

impl UnknownReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unsupported => "Unsupported",
            Self::NotChecked => "Not checked",
            Self::VersionUnknown => "Version unknown",
            Self::VersionUnclear => "Version unclear",
            Self::CheckFailed => "Check failed",
            Self::IdentityUnresolved => "Identity unresolved",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Unsupported => "The detected provider lists installations but does not check update candidates. Publisher release notes are separate.",
            Self::NotChecked => "A manager update check has not been requested for every installation in this group.",
            Self::VersionUnknown => "An installed version is missing, so this installation's update state cannot be established.",
            Self::VersionUnclear => "A provider returned a candidate, but its precedence over the installed version cannot be established safely.",
            Self::CheckFailed => "A manager update check failed or was incomplete. This does not mean the installation is current.",
            Self::IdentityUnresolved => "An observation's update identity or its coverage by another manager has not been verified. Equal names or versions alone are insufficient.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Assessment {
    pub update: Update,
    pub reason: Option<UnknownReason>,
}

impl Assessment {
    pub fn label(self) -> &'static str {
        match self.update {
            Update::Available => "Update available",
            Update::Current => "Current in manager",
            _ => self.reason.unwrap_or(UnknownReason::NotChecked).label(),
        }
    }

    pub fn detail(self) -> &'static str {
        match self.update {
            Update::Available => "A manager reported a newer candidate for at least one installation. This is not a claim about every upstream release or installation compatibility.",
            Update::Current => "No newer candidate was reported by the successful manager checks covering this group. This does not establish that the publisher has no newer release.",
            _ => self.reason.unwrap_or(UnknownReason::NotChecked).detail(),
        }
    }
}

pub fn assessment(s: &State, r: &PackageRow) -> Assessment {
    let mut checked = false;
    let mut reason = None;
    let mut record_reason = |value| reason = Some(reason.map_or(value, |old: UnknownReason| old.max(value)));
    for &i in &r.members {
        let p = &s.catalog.packages[i];
        let Some(o) = &p.observation else {
            record_reason(if p.provider.is_some() { UnknownReason::IdentityUnresolved } else { UnknownReason::NotChecked });
            continue;
        };
        if o.confirmed_update {
            return Assessment { update: Update::Available, reason: None };
        }
        if p.provider == Some(Manager::Local)
            && o.registration_id.as_deref().is_some_and(|receipt| {
                receipt.starts_with("ARP\\") && receipt == o.id
                    && r.members.iter().any(|&other| {
                        let other = &s.catalog.packages[other];
                        other.provider.is_some_and(releasescout_engine::inventory_refresh::supports_candidates)
                            && other.observation.as_ref().is_some_and(|managed| {
                                managed.requested && managed.succeeded
                                    && managed.registration_id.as_deref() == Some(receipt)
                            })
                            && p.installed.as_deref().is_some_and(|v| !v.trim().is_empty()
                                && other.installed.as_deref() == Some(v))
                    })
            })
        {
            // Exact ARP receipt/version coverage, not a same-name or same-version merge.
            continue;
        }
        if !p.provider.is_some_and(releasescout_engine::inventory_refresh::supports_candidates) {
            record_reason(UnknownReason::Unsupported);
            continue;
        }
        if !o.requested {
            record_reason(UnknownReason::NotChecked);
            continue;
        }
        if !o.succeeded {
            record_reason(if o.detail.as_deref().is_some_and(|d|d.starts_with("ambiguous or stale update identity:")) {
                UnknownReason::IdentityUnresolved
            } else { UnknownReason::CheckFailed });
            continue;
        }
        if p.installed.as_deref().is_none_or(|v| v.trim().is_empty()) {
            record_reason(UnknownReason::VersionUnknown);
            continue;
        }
        let comparable = match (&p.installed, &p.target) {
            (Some(_), None) => true,
            (Some(from), Some(to)) => candidate_order(from, to).is_some_and(|order|!order.is_gt()),
            _ => false,
        };
        checked |= comparable;
        if !comparable { record_reason(UnknownReason::VersionUnclear); }
    }
    if checked && reason.is_none() {
        Assessment { update: Update::Current, reason: None }
    } else {
        let reason = match reason {
            Some(UnknownReason::Unsupported) if checked => UnknownReason::IdentityUnresolved,
            Some(reason) => reason,
            None => UnknownReason::NotChecked,
        };
        Assessment { update: Update::Unknown, reason: Some(reason) }
    }
}

pub fn update(s: &State, r: &PackageRow) -> Update {
    assessment(s, r).update
}

pub fn source(s: &State, p: &Package) -> bool {
    // Installed extension changelog adapters are sources even without a catalog stream.
    p.extension.is_some()
        || notes(s, p)
        || s.live.catalog.packages.iter().any(|profile| {
            profile.id == p.id
                && matches!(profile.tracking.as_str(), "track" | "low_priority")
                && profile
                    .streams
                    .iter()
                    .any(|stream| !stream.sources.is_empty())
        })
}

pub fn notes(s: &State, p: &Package) -> bool {
    !p.sample_notes
        && s.live
            .sessions
            .get(&p.id)
            .and_then(|q| q.report.as_ref())
            .is_some_and(|r| {
                r.notes
                    .iter()
                    .any(|n| n.authored_notes && !n.body.trim().is_empty())
            })
}

pub fn matches(s: &State, r: &PackageRow) -> bool {
    let f = &s.activity.data.sidebar_filters[s.section as usize];
    (f.update == Update::Any || f.update == update(s, r))
        && f.source.is_none_or(|wanted| {
            r.members.iter().any(|&i| source(s, &s.catalog.packages[i])) == wanted
        })
        && f.notes.is_none_or(|wanted| {
            r.members.iter().any(|&i| notes(s, &s.catalog.packages[i])) == wanted
        })
        && (f.providers.is_empty()
            || r.members.iter().any(|&i| {
                f.providers
                    .contains(provider(&s.catalog.packages[i]).map_or("unrecorded", Manager::id))
            }))
}

pub fn count(s: &State) -> usize {
    let f = &s.activity.data.sidebar_filters[s.section as usize];
    usize::from(f.update != Update::Any)
        + usize::from(f.source.is_some())
        + usize::from(f.notes.is_some())
        + usize::from(!f.providers.is_empty())
        + usize::from(s.section == LibrarySection::Software && s.show_components)
}

impl State {
    pub fn change_sidebar_filter(&mut self, change: impl FnOnce(&mut Facets), tx: &mut Tx) {
        let revealed = self.command.revealed.take().is_some();
        change(&mut self.activity.data.sidebar_filters[self.section as usize]);
        self.activity.dirty = true;
        self.description_hover = None;
        if revealed {
            self.set_dataset(self.dataset);
        } else {
            self.refilter();
        }
        tx.scroll_to_instant("viewport", 0., 0.);
        self.status = self.inventory_summary();
    }
}

static FILTER: Icon = Icon {
    name: "sidebar-filter",
    view: 24.,
    body: include_str!("../assets/symbols/list-filter.svg"),
};
static SOURCE: Icon = Icon {
    name: "filter-source",
    view: 24.,
    body: include_str!("../assets/status/link-2.svg"),
};
static NOTES: Icon = Icon {
    name: "filter-notes",
    view: 24.,
    body: include_str!("../assets/status/file-text.svg"),
};
static PROVIDERS: Icon = Icon {
    name: "filter-providers",
    view: 24.,
    body: include_str!("../assets/symbols/package.svg"),
};
static RESET: Icon = Icon {
    name: "filter-reset",
    view: 24.,
    body: include_str!("../assets/status/rotate-ccw.svg"),
};

pub fn trigger(s: &State, k: &Kit) -> El {
    let active = count(s);
    row()
        .key("sidebar-filters-trigger")
        .role("button")
        .label("Filters")
        .description(format!("{active} active filters"))
        .help("Filters")
        .expanded(s.sidebar_filters_open)
        .focusable(true)
        .h(22.)
        .min_w(22.)
        .px(4.)
        .gap(4.)
        .center_y()
        .radius(4.)
        .fill(if active > 0 {
            k.c.layer_02
        } else {
            rune::Color::TRANSPARENT
        })
        .hover(k.c.hover)
        .focus_ring(1., 0., k.c.border_focus)
        .on_press(|s: &mut State| {
            s.sidebar_filters_open = !s.sidebar_filters_open;
            s.sidebar_sort_open = false;
            s.description_hover = None;
        })
        .child(
            icon(&FILTER, 14.)
                .color(if active > 0 {
                    k.c.text_base
                } else {
                    k.c.text_muted
                })
                .a11y_hidden(),
        )
        .children((active > 0).then(|| {
            text(active.to_string())
                .key("sidebar-filter-count")
                .font(k.ty.text_xsmall)
                .color(k.c.text_base)
                .a11y_hidden()
        }))
}

fn title(label: &str, k: &Kit) -> El {
    text(label)
        .font(TextStyle::new(12., 500.).lh(16.))
        .color(k.c.text_base)
}

fn heading(key: &str, label: &str, glyph: &'static Icon, k: &Kit) -> El {
    let heading = row()
        .key(format!("{key}-heading"))
        .w_full()
        .gap(6.)
        .center_y()
        .child(
            icon(glyph, 14.)
                .key(format!("{key}-icon"))
                .color(k.c.icon_muted)
                .a11y_hidden(),
        )
        .child(title(label, k).key(format!("{key}-label")));
    if matches!(key, "filter-update" | "filter-source" | "filter-notes") {
        heading.justify_center()
    } else {
        heading
    }
}

fn divider(key: &str, k: &Kit) -> El {
    el().key(key)
        .w_full()
        .h(1.)
        .my(10.)
        .fill(k.c.border_base)
        .a11y_hidden()
}

fn toggle_provider(s: &mut State, id: &str, tx: &mut Tx) {
    s.change_sidebar_filter(
        |f| {
            if !f.providers.remove(id) {
                f.providers.insert(id.to_owned());
            }
        },
        tx,
    );
}

fn ternary(
    key: &str,
    label: &str,
    labels: &[&str],
    value: Option<bool>,
    field: fn(&mut Facets) -> &mut Option<bool>,
    k: &Kit,
) -> El {
    col()
        .gap(6.)
        .child(heading(
            key,
            label,
            if key == "filter-source" {
                &SOURCE
            } else {
                &NOTES
            },
            k,
        ))
        .child(
            SegmentedControl::new(
                labels,
                Some(match value {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                }),
            )
            .width(None)
            .label(label)
            .on_pick(move |s: &mut State, index, tx| {
                s.change_sidebar_filter(
                    |f| {
                        *field(f) = match index {
                            1 => Some(true),
                            2 => Some(false),
                            _ => None,
                        }
                    },
                    tx,
                );
            })
            .build(k)
            .key(key),
        )
}

pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>) {
    if !s.sidebar_filters_open {
        return;
    }
    let f = &s.activity.data.sidebar_filters[s.section as usize];
    let mut segment_kit = k.clone();
    segment_kit.ty.ui = k.ty.text_xsmall;
    let active = count(s);
    let header = row()
        .key("filter-panel-header")
        .w_full()
        .px(12.)
        .py(8.)
        .gap(6.)
        .center_y()
        .border_sides([0., 0., 1., 0.], k.c.border_base)
        .child(icon(&FILTER, 14.).color(k.c.icon_muted).a11y_hidden())
        .child(text("Filters").font(k.ty.ui_medium).color(k.c.text_base))
        .children((active > 0).then(|| {
            row()
                .key("filter-active-badge")
                .h(18.)
                .min_w(18.)
                .px(5.)
                .radius(4.)
                .fill(k.c.layer_02)
                .center()
                .label(format!("{active} active filters"))
                .child(
                    text(active.to_string())
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_base),
                )
        }))
        .child(el().grow(1.))
        .child(
            Button::new("Reset")
                .icon(&RESET)
                .disabled(active == 0)
                .on_press_tx(|s: &mut State, tx| {
                    let support = s.section == LibrarySection::Software && s.show_components;
                    s.change_sidebar_filter(|f| *f = Facets::default(), tx);
                    if support {
                        s.toggle_components();
                    }
                })
                .build(k)
                .key("sidebar-filters-reset"),
        );
    let mut content = col().key("filter-content").w_full().padding(12.).child(
        col()
            .gap(10.)
            .child(
                col()
                    .gap(6.)
                    .child(heading(
                        "filter-update",
                        if s.section == LibrarySection::Software { "Manager updates" } else { "Updates" },
                        &crate::sidebar_info::DOWNLOAD,
                        k,
                    ))
                    .child(
                        SegmentedControl::new(
                            &["All", "Available", "Current", "Unknown"],
                            Some(f.update as usize),
                        )
                        .width(None)
                        .label("Update status")
                        .on_pick(|s: &mut State, index, tx| {
                            s.change_sidebar_filter(
                                |f| {
                                    f.update = [
                                        Update::Any,
                                        Update::Available,
                                        Update::Current,
                                        Update::Unknown,
                                    ][index]
                                },
                                tx,
                            );
                        })
                        .build(&segment_kit)
                        .key("filter-update"),
                    ),
            )
            .child(ternary(
                "filter-source",
                "Release source",
                &["All", "Found", "Missing"],
                f.source,
                |f| &mut f.source,
                &segment_kit,
            ))
            .child(ternary(
                "filter-notes",
                "Notes",
                &["All", "Loaded", "Not loaded"],
                f.notes,
                |f| &mut f.notes,
                &segment_kit,
            )),
    );
    let mut providers = col()
        .key("filter-provider-section")
        .gap(2.)
        .child(heading("filter-providers", "Providers", &PROVIDERS, k).mb(6.));
    let mut options = Vec::new();
    for manager in Manager::ALL {
        if s.rows.iter().any(|r| {
            r.members
                .iter()
                .any(|&i| provider(&s.catalog.packages[i]) == Some(manager))
        }) || f.providers.contains(manager.id())
        {
            options.push((
                manager.id().to_owned(),
                crate::provider_badge::label(manager).to_owned(),
                Some(manager),
            ));
        }
    }
    if s.rows.iter().any(|r| {
        r.members
            .iter()
            .any(|&i| provider(&s.catalog.packages[i]).is_none())
    }) || f.providers.contains("unrecorded")
    {
        options.push(("unrecorded".into(), "Unrecorded".into(), None));
    }
    for pair in options.chunks(2) {
        let mut line = row().w_full().gap(6.);
        for (id, label, manager) in pair {
            let value = id.clone();
            let row_value = id.clone();
            let check = Checkbox::new(f.providers.contains(id))
                .hide_label()
                .label(label)
                .on_toggle_tx(move |s: &mut State, tx| {
                    toggle_provider(s, &value, tx);
                })
                .build(k)
                .key(format!("filter-provider-{id}"));
            line = line.child(
                row()
                    .key(format!("filter-provider-row-{id}"))
                    .w(130.)
                    .grow(1.)
                    .min_w(0.)
                    .gap(6.)
                    .py(4.)
                    .radius(4.)
                    .hover(k.c.hover)
                    .on_press_tx(move |s: &mut State, tx| toggle_provider(s, &row_value, tx))
                    .center_y()
                    .child(check)
                    .children(manager.map(|m| crate::provider_badge::symbol(m, k)))
                    .child(
                        text(label)
                            .font(k.ty.text_xsmall)
                            .color(k.c.text_base)
                            .truncate(),
                    ),
            );
        }
        providers = providers.child(line);
    }
    content = content
        .child(divider("filter-provider-divider", k))
        .child(providers);
    if s.section == LibrarySection::Software && s.dataset == crate::model::Dataset::Detected {
        content = content.child(divider("filter-system-divider", k)).child(
            row()
                .key("filter-system-row")
                .w_full()
                .center_y()
                .justify_between()
                .child(title("System & support", k))
                .child(crate::sidebar::system_switch(s, k)),
        );
    }
    let footer = row()
        .key("filter-panel-footer")
        .w_full()
        .px(12.)
        .py(8.)
        .border_sides([1., 0., 0., 0.], k.c.border_base)
        .child(
            text(format!("{} of {} shown", s.visible.len(), s.rows.len()))
                .key("filter-results")
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted),
        );
    let close = |s: &mut State| s.sidebar_filters_open = false;
    let surface = popover::raised(
        k,
        col()
            .key("sidebar-filters")
            .role("dialog")
            .label("Filters")
            .focus_group()
            .focus_return("sidebar-filters-trigger")
            .w(352.)
            .min_h(0.)
            .child(header)
            .child(
                scroll_bar(
                    k,
                    "filters-scroll-area",
                    "filter-scroll",
                    popover::fit_scroll(content).key("filter-scroll"),
                )
                .w_full()
                .shrink()
                .min_h(0.),
            )
            .child(footer),
        6.,
    )
    .border(1., k.c.border_base)
    .shadow(4., 12., rune::Color::BLACK.alpha(0.14))
    .modal(true, close);
    layers.push(scrim("sidebar-filters-scrim", close));
    layers.push(popover::anchored(
        k,
        surface,
        "sidebar-filters-trigger",
        Placement::BottomEnd,
        4.,
    ));
}

pub fn scene(s: &mut State) {
    crate::sidebar_info::scene(s);
    s.sidebar_filters_open = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facets_intersect_without_mutating_inventory_and_loaded_notes_are_not_source_hints() {
        let mut s = State::new();
        crate::sidebar_info::scene(&mut s);
        let inventory = s.catalog.packages.len();
        let row_count = s.rows.len();
        let names = |s: &State| {
            s.visible
                .iter()
                .map(|&i| s.rows[i].name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        let unknown = s
            .rows
            .iter()
            .find(|r| r.name == "Unmapped Fixture Tool")
            .unwrap()
            .package;
        s.catalog.packages[unknown]
            .observation
            .as_mut()
            .unwrap()
            .succeeded = false;
        s.activity.data.sidebar_filters[0].update = Update::Unknown;
        s.refilter();
        assert_eq!(names(&s), "Unmapped Fixture Tool");
        s.activity.data.sidebar_filters[0] = Facets {
            update: Update::Available,
            source: Some(true),
            notes: Some(true),
            ..Default::default()
        };
        s.refilter();
        assert_eq!(names(&s), "GitHub CLI");
        s.activity.data.sidebar_filters[0]
            .providers
            .insert("winget".into());
        s.refilter();
        assert!(s.visible.is_empty());
        s.activity.data.sidebar_filters[0]
            .providers
            .insert("scoop".into());
        s.refilter();
        assert_eq!(
            names(&s),
            "GitHub CLI",
            "Providers are OR; independent facets are AND"
        );
        assert_eq!(s.catalog.packages.len(), inventory);
        assert_eq!(s.rows.len(), row_count);
        s.activity.data.sidebar_filters[0] = Facets {
            source: Some(false),
            notes: Some(false),
            ..Default::default()
        };
        s.refilter();
        assert_eq!(names(&s), "Unmapped Fixture Tool");
        assert!(s.selected().is_some());
    }

    #[test]
    fn nonstandard_inventory_candidates_are_available_but_unordered_channels_stay_unknown() {
        for (from, to, expected) in [
            ("r40", "r42", Update::Available),
            ("8.18.0_2", "8.23.0_9", Update::Available),
            ("< 3.14.7", "3.15.2", Update::Available),
            ("< 4.5.26.3894", "4.5.28.3936", Update::Available),
            ("1.2.65.255.g85e641b4", "1.3.9.2055.g806b5b278de8", Update::Available),
            ("r42", "r40", Update::Current),
            ("nightly", "latest", Update::Unknown),
        ] {
            let mut s = State::new();
            let data = serde_json::json!({
                "installed":{"apps":[{"Name":"fixture-tool","Version":from,"Source":"main"}]},
                "updates":[{"Name":"fixture-tool","Installed Version":from,"Latest Version":to}]
            });
            let mut scan = releasescout_engine::inventory::parse(Manager::Scoop, true, &data.to_string()).unwrap();
            s.accept_scan(scan.clone());
            let row = s.rows.first().unwrap();
            assert_eq!(update(&s, row), expected, "{from} -> {to}");
            s.activity.data.sidebar_filters[0] = Facets { update: Update::Unknown, ..Default::default() };
            s.refilter();
            assert_eq!(s.visible.len(), usize::from(expected == Update::Unknown),
                "Unknown filter disagrees with the candidate status for {from} -> {to}");
            scan.packages[0].candidate_check_succeeded = Some(false);
            s.accept_scan(scan.clone());
            assert_eq!(update(&s, s.rows.first().unwrap()), Update::Unknown);
            scan.update_check_succeeded = false;
            scan.packages[0].candidate_check_succeeded = None;
            s.accept_scan(scan.clone());
            assert_eq!(update(&s, s.rows.first().unwrap()), Update::Unknown, "legacy failed scans must not confirm a candidate");
            scan.packages[0].candidate_check_succeeded = Some(true);
            s.accept_scan(scan);
            assert_eq!(update(&s, s.rows.first().unwrap()), expected, "successful per-record checks survive an unrelated scan failure");
        }
    }

    #[test]
    fn only_exact_local_receipts_with_matching_versions_share_a_successful_manager_check() {
        let mut s = State::new();
        crate::sidebar_info::scene(&mut s);
        let original = s.rows.iter().find(|r|r.name=="pnpm").unwrap();
        let mut row = PackageRow { id: original.id.clone(), name: original.name.clone(), package: original.package, members: original.members.clone() };
        let managed = &mut s.catalog.packages[row.package];
        managed.observation.as_mut().unwrap().registration_id = Some("ARP\\User\\X64\\fixture-tool".into());
        let mut local = managed.clone();
        local.id = "fixture-local-observation".into();
        local.provider = Some(Manager::Local);
        let observation = local.observation.as_mut().unwrap();
        observation.id = "ARP\\User\\X64\\fixture-tool".into();
        observation.requested = false;
        observation.succeeded = false;
        row.members.push(s.catalog.packages.len());
        s.catalog.packages.push(local.clone());
        assert_eq!(update(&s, &row), Update::Current);
        let index = *row.members.last().unwrap();
        s.catalog.packages[index].installed = Some("0.0.1".into());
        assert_eq!(update(&s, &row), Update::Unknown, "different installed versions must not share checks");
        s.catalog.packages[index] = local.clone();
        s.catalog.packages[index].observation.as_mut().unwrap().registration_id = Some("ARP\\Machine\\X64\\other-install".into());
        assert_eq!(update(&s, &row), Update::Unknown, "same name/version is not an exact receipt");
        s.catalog.packages[index] = local.clone();
        s.catalog.packages[row.package].observation.as_mut().unwrap().succeeded = false;
        assert_eq!(update(&s, &row), Update::Unknown, "failed checks cannot cover registrations");
        s.catalog.packages[row.package].observation.as_mut().unwrap().succeeded = true;
        s.catalog.packages[index] = local;
        s.catalog.packages[index].provider = Some(Manager::Store);
        assert_eq!(update(&s, &row), Update::Unknown, "Store identity needs separate family evidence");
    }

    #[test]
    fn grouped_providers_use_every_observation_but_current_requires_all_to_be_checked() {
        let mut s = State::new();
        crate::sidebar_info::scene(&mut s);
        let original = s.rows.iter().find(|r| r.name == "pnpm").unwrap();
        let mut r = PackageRow {
            id: original.id.clone(),
            name: original.name.clone(),
            package: original.package,
            members: original.members.clone(),
        };
        assert_eq!(update(&s, &r), Update::Current);
        let mut other = s.catalog.packages[r.package].clone();
        other.id = "other-install".into();
        other.provider = Some(Manager::Winget);
        other.observation.as_mut().unwrap().succeeded = false;
        r.members.push(s.catalog.packages.len());
        s.catalog.packages.push(other);
        assert_eq!(update(&s, &r), Update::Unknown);
        s.activity.data.sidebar_filters[0]
            .providers
            .insert("winget".into());
        assert!(matches(&s, &r));
        s.catalog.packages[r.package]
            .observation
            .as_mut()
            .unwrap()
            .confirmed_update = true;
        assert_eq!(update(&s, &r), Update::Available);
        s.catalog.packages[r.package]
            .observation
            .as_mut()
            .unwrap()
            .confirmed_update = false;
        s.catalog.packages[r.members[1]]
            .observation
            .as_mut()
            .unwrap()
            .succeeded = true;
        s.catalog.packages[r.members[1]].target = Some("not-comparable".into());
        assert_eq!(update(&s, &r), Update::Unknown);
    }
}
