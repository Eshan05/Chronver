use crate::{
    model::{PackageRow, State},
    rich_select::{OptionRow, RichSelect},
};
use chrono::{DateTime, NaiveDate};
use rune::{El, Icon, Tx, col, icon, row, text};
use rune_kit::{Kit, SortDir};
use std::{cmp::Ordering, collections::BTreeMap};

struct Choice {
    key: &'static str,
    label: &'static str,
    direction: SortDir,
}
static ASCENDING: Icon = Icon {
    name: "sort-ascending",
    view: 24.,
    body: include_str!("../assets/status/arrow-up-narrow-wide.svg"),
};
static DESCENDING: Icon = Icon {
    name: "sort-descending",
    view: 24.,
    body: include_str!("../assets/status/arrow-down-wide-narrow.svg"),
};
static NAME: Icon = Icon {
    name: "sort-name",
    view: 24.,
    body: include_str!("../assets/status/case-sensitive.svg"),
};
static DATE: Icon = Icon {
    name: "sort-date",
    view: 24.,
    body: include_str!("../assets/status/calendar-days.svg"),
};
static CHECKED: Icon = Icon {
    name: "sort-checked",
    view: 24.,
    body: include_str!("../assets/status/history.svg"),
};
static PROVIDER: Icon = Icon {
    name: "sort-provider",
    view: 24.,
    body: include_str!("../assets/symbols/package.svg"),
};

fn choice_icon(choice: &Choice) -> &'static Icon {
    match choice.key {
        "date" => &DATE,
        "checked" => &CHECKED,
        "update" => &crate::sidebar_info::DOWNLOAD,
        "manager" => &PROVIDER,
        _ => &NAME,
    }
}
const CHOICES: &[Choice] = &[
    Choice {
        key: "name",
        label: "Name",
        direction: SortDir::Ascending,
    },
    Choice {
        key: "date",
        label: "Release date",
        direction: SortDir::Descending,
    },
    Choice {
        key: "checked",
        label: "Last checked",
        direction: SortDir::Descending,
    },
    Choice {
        key: "update",
        label: "Download available",
        direction: SortDir::Descending,
    },
    Choice {
        key: "manager",
        label: "Provider",
        direction: SortDir::Ascending,
    },
];

fn current(s: &State) -> (&'static Choice, SortDir) {
    let (key, dir) = s.table.sort().unwrap_or(("name", SortDir::Ascending));
    (
        CHOICES.iter().find(|c| c.key == key).unwrap_or(&CHOICES[0]),
        dir,
    )
}

fn number(s: &State, r: &PackageRow, column: &str) -> Option<i64> {
    let today = crate::sidebar_info::today();
    match column {
        "date" => r
            .members
            .iter()
            .filter_map(|&i| {
                let p = &s.catalog.packages[i];
                if p.sample_notes {
                    return None;
                }
                s.live
                    .sessions
                    .get(&p.id)?
                    .report
                    .as_ref()?
                    .notes
                    .iter()
                    .filter(|n| n.authored_notes && !n.body.trim().is_empty())
                    .filter_map(|n| n.date)
                    .filter(|d| *d <= today)
                    .max()
            })
            .max()
            .map(|d| {
                d.signed_duration_since(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
                    .num_days()
            }),
        "checked" => r
            .members
            .iter()
            .flat_map(|&i| {
                let p = &s.catalog.packages[i];
                let source = s
                    .live
                    .sessions
                    .get(&p.id)
                    .and_then(|q| q.report_feed.as_ref())
                    .and_then(|feed| s.activity.data.source_checks.get(feed));
                p.observation
                    .as_ref()
                    .map(|o| o.checked_at.as_str())
                    .into_iter()
                    .chain(source.map(String::as_str))
            })
            .filter_map(|time| DateTime::parse_from_rfc3339(time).ok())
            .filter(|time| time.date_naive() <= today)
            .map(|time| time.timestamp())
            .max(),
        "update" => {
            let observations: Vec<_> = r
                .members
                .iter()
                .filter_map(|&i| s.catalog.packages[i].observation.as_ref())
                .collect();
            if observations.iter().any(|o| o.confirmed_update) {
                Some(1)
            } else if observations.iter().any(|o| o.requested && o.succeeded) {
                Some(0)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn known_first(a: Option<i64>, b: Option<i64>, dir: SortDir) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => dir.apply(a.cmp(&b)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

pub fn sort(s: &mut State) {
    let (choice, dir) = current(s);
    // Compute report/date keys once per row, never inside the comparison loop.
    let numbers: BTreeMap<_, _> = if matches!(choice.key, "date" | "checked" | "update") {
        s.visible
            .iter()
            .map(|&i| (i, number(s, &s.rows[i], choice.key)))
            .collect()
    } else {
        BTreeMap::new()
    };
    let providers: BTreeMap<_, _> = if choice.key == "manager" {
        s.visible
            .iter()
            .map(|&i| {
                let r = &s.rows[i];
                let mut labels: Vec<_> = crate::provider_badge::unique(
                    r.members
                        .iter()
                        .filter_map(|&i| s.catalog.packages[i].provider),
                )
                .into_iter()
                .map(crate::provider_badge::label)
                .collect();
                labels.sort_unstable();
                (
                    i,
                    if labels.is_empty() {
                        s.catalog.packages[r.package].manager_label().into()
                    } else {
                        labels.join(" / ")
                    },
                )
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    s.visible.sort_by(|&a, &b| {
        let names = || rune_kit::natural_cmp(&s.rows[a].name, &s.rows[b].name);
        let primary = match choice.key {
            "date" | "checked" | "update" => known_first(numbers[&a], numbers[&b], dir),
            "manager" => dir.apply(providers[&a].cmp(&providers[&b])),
            _ => dir.apply(names()),
        };
        primary
            .then_with(names)
            .then_with(|| s.rows[a].id.cmp(&s.rows[b].id))
    });
}

pub fn controls(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    let (choice, dir) = current(s);
    let select = RichSelect {
        key: "sidebar-sort",
        label: "Sort library",
        current: choice.key,
        open: s.sidebar_sort_open,
        popup_width: 216.,
        value: row()
            .min_w(0.)
            .gap(6.)
            .center_y()
            .child(
                icon(choice_icon(choice), 14.)
                    .color(k.c.icon_muted)
                    .key("sidebar-sort-value-icon"),
            )
            .child(
                text(if choice.key == "update" {
                    "Updates"
                } else {
                    choice.label
                })
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted)
                .grow(1.)
                .min_w(0.)
                .truncate(),
            ),
        options: CHOICES
            .iter()
            .map(|c| OptionRow {
                id: c.key.into(),
                label: c.label.into(),
                description: String::new(),
                content: row()
                    .w_full()
                    .min_w(0.)
                    .gap(8.)
                    .center_y()
                    .child(
                        icon(choice_icon(c), 16.)
                            .color(k.c.icon_muted)
                            .key(format!("sidebar-sort-icon-{}", c.key)),
                    )
                    .child(
                        text(c.label)
                            .font(k.ty.ui)
                            .color(k.c.text_base)
                            .grow(1.)
                            .min_w(0.)
                            .truncate(),
                    ),
            })
            .collect(),
        on_toggle: |s: &mut State| {
            s.sidebar_sort_open = !s.sidebar_sort_open;
            s.sidebar_filters_open = false;
            s.description_hover = None;
        },
        on_close: |s: &mut State| s.sidebar_sort_open = false,
        on_choose: |s: &mut State, key: &str, tx: &mut Tx| {
            if let Some(choice) = CHOICES.iter().find(|c| c.key == key) {
                s.table.set_sort(choice.key, choice.direction);
                s.sort_visible();
                tx.scroll_to_instant("viewport", 0., 0.);
            }
        },
    }
    .build_compact(k, layers);
    row()
        .w_full()
        .min_w(0.)
        .gap(4.)
        .center_y()
        .child(col().grow(1.).min_w(0.).child(select))
        .child(
            crate::sidebar_actions::action(
                "sidebar-sort-direction",
                if dir == SortDir::Ascending {
                    &ASCENDING
                } else {
                    &DESCENDING
                },
                if dir == SortDir::Ascending {
                    "Sort descending"
                } else {
                    "Sort ascending"
                },
                k,
            )
            .on_press_tx(|s: &mut State, tx| {
                let (choice, dir) = current(s);
                s.table.set_sort(
                    choice.key,
                    if dir == SortDir::Ascending {
                        SortDir::Descending
                    } else {
                        SortDir::Ascending
                    },
                );
                s.sort_visible();
                tx.scroll_to_instant("viewport", 0., 0.);
            }),
        )
}

pub fn scene(s: &mut State) {
    crate::sidebar_info::scene(s);
    s.sidebar_sort_open = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_dates_stay_last_in_both_directions() {
        for dir in [SortDir::Ascending, SortDir::Descending] {
            assert_eq!(known_first(Some(1), None, dir), Ordering::Less);
            assert_eq!(known_first(None, Some(1), dir), Ordering::Greater);
        }
        assert_eq!(
            known_first(Some(1), Some(2), SortDir::Descending),
            Ordering::Greater
        );
    }
    #[test]
    fn grouped_rows_use_all_observations_and_confirmed_candidates() {
        let mut s = State::new();
        crate::sidebar_info::scene(&mut s);
        let gh = s.rows.iter().find(|r| r.name == "GitHub CLI").unwrap();
        assert_eq!(number(&s, gh, "update"), Some(1));
        assert_eq!(
            number(&s, gh, "date"),
            Some(
                (crate::sidebar_info::today() - chrono::Duration::days(3))
                    .signed_duration_since(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
                    .num_days()
            )
        );
        let mut grouped = PackageRow {
            id: "test-group".into(),
            name: "Test".into(),
            package: gh.package,
            members: vec![gh.package],
        };
        let candidate = gh.package;
        let other = s.rows.iter().find(|r| r.name == "pnpm").unwrap().package;
        grouped.members.insert(0, other);
        grouped.package = other;
        assert_eq!(number(&s, &grouped, "update"), Some(1));
        s.catalog.packages[candidate]
            .observation
            .as_mut()
            .unwrap()
            .confirmed_update = false;
        s.catalog.packages[candidate]
            .observation
            .as_mut()
            .unwrap()
            .requested = false;
        s.catalog.packages[other]
            .observation
            .as_mut()
            .unwrap()
            .requested = false;
        assert_eq!(number(&s, &grouped, "update"), None);
        s.catalog.packages[candidate].sample_notes = true;
        s.catalog.packages[other].sample_notes = true;
        assert_eq!(number(&s, &grouped, "date"), None);
    }
}
