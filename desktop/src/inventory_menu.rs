use crate::{
    inventory_workspace::{self as workspace, Record},
    model::State,
    package_info,
};
use rune::{El, Icon, PointerPress, Tx, el, row};
use rune_kit::{Kit, MenuItem, context_menu, glyphs};

static INFO: Icon = Icon {
    name: "inventory-menu-info",
    view: 24.,
    body: include_str!("../assets/status/info.svg"),
};
static SOURCE: Icon = Icon {
    name: "inventory-menu-source",
    view: 24.,
    body: include_str!("../assets/status/link-2.svg"),
};

#[derive(Clone, PartialEq)]
pub enum Target {
    Observation(String),
    Group(String),
}
impl Target {
    fn menu_key(&self) -> String {
        match self {
            Self::Observation(key) => format!("inventory-context-observation-{key}"),
            Self::Group(key) => format!("inventory-context-group-{key}"),
        }
    }
}
pub struct Open {
    pub target: Target,
    pub point: PointerPress,
}
#[derive(Clone, Copy)]
enum Action {
    CopyId,
    CopyVersion,
    Source,
    Details,
    Review,
    SelectGroup,
    ToggleGroup,
}

fn source(s: &State, r: &Record<'_>) -> Option<String> {
    r.linked
        .and_then(|p| package_info::source(s, p))
        .into_iter()
        .chain(r.package.release_notes.iter().cloned())
        .chain(r.package.repository.iter().cloned())
        .chain(r.package.homepage.iter().cloned())
        .find_map(|raw| package_info::public_url(&raw).map(Into::into))
}

fn close(s: &mut State) {
    s.inventory.workspace.context = None;
}

pub fn scene(s: &mut State) {
    crate::inventory_view::workspace_scene(s);
    let key = workspace::records(s)
        .into_iter()
        .find(|r| r.package.id == "gh")
        .unwrap()
        .key;
    s.inventory.workspace.context = Some(Open {
        target: Target::Observation(key),
        point: PointerPress { x: 800., y: 440. },
    });
}

fn act(s: &mut State, target: &Target, action: Action, tx: &mut Tx) {
    // Resolve the identity again: a refresh may have replaced the saved observation.
    if !s.inventory.open
        || !s
            .inventory
            .workspace
            .context
            .as_ref()
            .is_some_and(|o| &o.target == target)
    {
        return;
    }
    close(s);
    match target {
        Target::Observation(key) => {
            let Some(record) = workspace::filtered(s).into_iter().find(|r| &r.key == key) else {
                return;
            };
            match action {
                Action::CopyId => {
                    tx.copy(record.package.id.clone());
                    s.status = "Package ID copied".into();
                }
                Action::CopyVersion => {
                    if let Some(version) = record
                        .package
                        .installed
                        .as_deref()
                        .filter(|v| !v.trim().is_empty())
                    {
                        tx.copy(version.to_owned());
                        s.status = "Installed version copied".into();
                    }
                }
                Action::Source => {
                    if let Some(url) = source(s, &record) {
                        s.status = match tx.open_url(&url) {
                            Ok(()) => "Opened source in browser".into(),
                            Err(error) => format!("Could not open source: {error}"),
                        };
                    }
                }
                Action::Details => {
                    s.inventory.details = Some(key.clone());
                    tx.scroll_into_view(format!("inventory-details-{key}"));
                }
                Action::Review => {
                    let review = workspace::review_rows(vec![record]);
                    if review.items[0].command.is_ok() {
                        s.inventory.workspace.review = Some(review);
                    }
                }
                _ => {}
            }
        }
        Target::Group(key) => {
            let rows = workspace::filtered(s);
            if !workspace::groups(&rows)
                .iter()
                .any(|g| &g.key == key && g.members.len() > 1)
            {
                return;
            }
            match action {
                Action::SelectGroup => s.toggle_inventory_group_selection(key, tx),
                Action::ToggleGroup if !s.inventory.workspace.collapsed_groups.remove(key) => {
                    s.inventory.workspace.collapsed_groups.insert(key.clone());
                }
                _ => {}
            }
        }
    }
    tx.instant();
}

fn item(target: &Target, action: Action, label: &str, icon: &'static Icon) -> MenuItem {
    let target = target.clone();
    MenuItem::new(label)
        .icon(icon)
        .on_select_tx(move |s: &mut State, tx| act(s, &target, action, tx))
}

pub fn trigger(target: Target, trigger: El, k: &Kit) -> El {
    let key = target.menu_key();
    let anchor = format!("{key}-trigger");
    row()
        .key(&anchor)
        .child(trigger)
        .w_full()
        .min_w(0.)
        .focusable(true)
        .focus_ring(1., -1., k.c.border_focus)
        .on_secondary_press(move |s: &mut State, point, tx| {
            if s.inventory.workspace.review.is_some() || s.inventory.workspace.export_busy {
                return;
            }
            s.inventory.workspace.columns_open = false;
            s.inventory.picker = false;
            s.inventory.workspace.context = Some(Open {
                target: target.clone(),
                point,
            });
            tx.focus(&anchor, false);
            tx.instant();
        })
}

pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>) {
    let Some(open) = &s.inventory.workspace.context else {
        return;
    };
    let rows = workspace::filtered(s);
    let target = &open.target;
    let mut menu = context_menu(target.menu_key(), el(), Some(open.point))
        .standard()
        .min_width(208.)
        .on_close(close);
    match target {
        Target::Observation(id) => {
            if let Some(r) = rows.iter().find(|r| &r.key == id) {
                let has_version = r
                    .package
                    .installed
                    .as_ref()
                    .is_some_and(|v| !v.trim().is_empty());
                let has_source = source(s, r).is_some();
                let can_review = workspace::review_rows(vec![r.clone()]).items[0]
                    .command
                    .is_ok();
                menu = menu
                    .item(item(
                        target,
                        Action::CopyId,
                        "Copy package ID",
                        &glyphs::COPY,
                    ))
                    .item(
                        item(
                            target,
                            Action::CopyVersion,
                            "Copy installed version",
                            &glyphs::COPY,
                        )
                        .disabled(!has_version),
                    )
                    .separator()
                    .item(
                        item(target, Action::Source, "Open source", &SOURCE).disabled(!has_source),
                    )
                    .item(item(
                        target,
                        Action::Details,
                        "Installation details",
                        &INFO,
                    ));
                if can_review {
                    menu = menu.separator().item(item(
                        target,
                        Action::Review,
                        "Review upgrade",
                        &crate::sidebar_info::UPDATE,
                    ));
                }
            } else {
                return;
            }
        }
        Target::Group(id) => {
            let all = rows
                .iter()
                .filter(|r| r.group_key() == id)
                .all(|r| s.inventory.workspace.selected.contains(&r.key));
            let collapsed = s.inventory.workspace.collapsed_groups.contains(id);
            menu = menu
                .item(item(
                    target,
                    Action::SelectGroup,
                    if all {
                        "Deselect group"
                    } else {
                        "Select group"
                    },
                    &glyphs::CHECK,
                ))
                .item(item(
                    target,
                    Action::ToggleGroup,
                    if collapsed {
                        "Expand group"
                    } else {
                        "Collapse group"
                    },
                    if collapsed {
                        &glyphs::CHEVRON_DOWN
                    } else {
                        &glyphs::CHEVRON_UP
                    },
                ));
        }
    }
    let _ = menu.build(&crate::dropdown::kit(k), layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_sources_use_the_observation_and_skip_unsafe_urls() {
        let mut s = State::new();
        crate::inventory_view::scene(&mut s);
        let mut scan = s.inventory.selected_scan().unwrap().clone();
        let p = scan
            .packages
            .iter_mut()
            .find(|p| p.id == "fixture-unknown")
            .unwrap();
        p.release_notes = Some("javascript:alert(1)".into());
        p.repository = Some("https://user:secret@example.org/".into());
        p.homepage = Some("https://example.org/fixture".into());
        s.accept_scan(scan.clone());
        let r = workspace::records(&s)
            .into_iter()
            .find(|r| r.package.id == "fixture-unknown")
            .unwrap();
        assert_eq!(
            source(&s, &r).as_deref(),
            Some("https://example.org/fixture")
        );
        scan.packages
            .iter_mut()
            .find(|p| p.id == "fixture-unknown")
            .unwrap()
            .homepage = Some("file:///C:/fixture.txt".into());
        s.accept_scan(scan);
        let r = workspace::records(&s)
            .into_iter()
            .find(|r| r.package.id == "fixture-unknown")
            .unwrap();
        assert!(source(&s, &r).is_none());
    }

    #[test]
    fn refresh_and_filter_reset_invalidate_open_context_actions() {
        let mut s = State::new();
        crate::inventory_view::workspace_scene(&mut s);
        let key = workspace::records(&s)
            .into_iter()
            .find(|r| r.package.id == "gh")
            .unwrap()
            .key;
        let target = Target::Observation(key);
        let open = || Open {
            target: target.clone(),
            point: PointerPress { x: 500., y: 400. },
        };
        s.inventory.workspace.context = Some(open());
        let scan = s.inventory.selected_scan().unwrap().clone();
        s.accept_scan(scan);
        assert!(s.inventory.workspace.context.is_none());
        act(&mut s, &target, Action::Review, &mut Tx::default());
        assert!(s.inventory.workspace.review.is_none());
        s.inventory.workspace.context = Some(open());
        s.reset_inventory_rows(&mut Tx::default());
        assert!(s.inventory.workspace.context.is_none());
        assert!(!s.inventory.work.is_pending());
    }
}
