use crate::{inventory_workspace as workspace, model::State, provider_badge, theme};
use releasescout_engine::inventory::Manager;
use rune::{El, col, el, row, text};
use rune_kit::{
    Button, ButtonVariant, Checkbox, IconButton, IconButtonVariant, Kit, glyphs, scroll_bar,
};
use std::collections::BTreeMap;

pub fn toggle_provider(s: &mut State, manager: Manager, enabled: bool, tx: &mut rune::Tx) {
    let w = &mut s.inventory.workspace;
    if enabled {
        w.hidden_providers.remove(manager.id());
        w.hidden_sources.retain(|(m, _)| m != manager.id());
    } else {
        w.hidden_providers.insert(manager.id().into());
    }
    s.reset_inventory_rows(tx);
}
pub fn view(s: &State, k: &Kit) -> El {
    let w = &s.inventory.workspace;
    let scans = workspace::scans(s);
    let mut list = col().w_full().gap(2.);
    for manager in Manager::ALL {
        let scan = scans.iter().find(|scan| scan.manager == manager);
        let mut sources = BTreeMap::<String, usize>::new();
        if let Some(scan) = scan {
            for p in &scan.packages {
                *sources.entry(p.source.clone()).or_default() += 1;
            }
        }
        let included = !w.hidden_providers.contains(manager.id());
        let all = included
            && sources.keys().all(|source| {
                !w.hidden_sources
                    .contains(&(manager.id().into(), source.clone()))
            });
        let any = included
            && (sources.is_empty()
                || sources.keys().any(|source| {
                    !w.hidden_sources
                        .contains(&(manager.id().into(), source.clone()))
                }));
        let expanded = w.expanded_providers.contains(manager.id());
        let mut group = row()
            .w_full()
            .min_w(0.)
            .gap(5.)
            .center_y()
            .py(3.)
            .child(
                Checkbox::new(all)
                    .indeterminate(any && !all)
                    .label(format!("Filter {}", provider_badge::label(manager)))
                    .hide_label()
                    .on_toggle_tx(move |s: &mut State, tx| toggle_provider(s, manager, !all, tx))
                    .build(k)
                    .key(format!("inventory-provider-{}", manager.id())),
            )
            .child(provider_badge::symbol_sized(manager, k, 15.))
            .child(
                text(provider_badge::label(manager))
                    .font(k.ty.ui_compact)
                    .color(k.c.text_base)
                    .grow(1.)
                    .min_w(0.)
                    .truncate()
                    .on_press_tx(move |s: &mut State, tx| toggle_provider(s, manager, !all, tx)),
            )
            .child(
                text(scan.map_or_else(|| "-".into(), |scan| scan.packages.len().to_string()))
                    .font(k.ty.text_xsmall)
                    .color(k.c.text_muted)
                    .help(scan.map_or_else(
                        || "Not scanned".into(),
                        |scan| format!("Last scan {}", crate::package_info::time(&scan.checked_at)),
                    )),
            );
        if !sources.is_empty() {
            group = group.child(
                IconButton::new(if expanded {
                    &glyphs::CHEVRON_UP
                } else {
                    &glyphs::CHEVRON_DOWN
                })
                .variant(IconButtonVariant::Ghost)
                .label(format!("Sources for {}", provider_badge::label(manager)))
                .help("Show provider sources")
                .expanded(expanded)
                .on_press_tx(move |s: &mut State, tx| {
                    if !s
                        .inventory
                        .workspace
                        .expanded_providers
                        .remove(manager.id())
                    {
                        s.inventory
                            .workspace
                            .expanded_providers
                            .insert(manager.id().into());
                    }
                    tx.instant();
                })
                .build(k)
                .size(20., 20.)
                .key(format!("inventory-provider-expand-{}", manager.id())),
            );
        } else {
            group = group.child(el().w(20.));
        }
        list = list.child(group);
        if expanded {
            for (source, count) in sources {
                let checked = included
                    && !w
                        .hidden_sources
                        .contains(&(manager.id().into(), source.clone()));
                let key = crate::activity::hash((manager.id(), &source));
                let label = if source.is_empty() || source.eq_ignore_ascii_case("unknown") {
                    "Unrecorded source"
                } else {
                    &source
                };
                let choice = source.clone();
                let text_choice = source.clone();
                list = list.child(
                    row()
                        .w_full()
                        .min_w(0.)
                        .pl(25.)
                        .gap(5.)
                        .py(2.)
                        .center_y()
                        .child(
                            Checkbox::new(checked)
                                .label(label)
                                .hide_label()
                                .on_toggle_tx(move |s: &mut State, tx| {
                                    s.toggle_inventory_source(manager, &choice, tx)
                                })
                                .build(k)
                                .w(24.)
                                .key(format!("inventory-source-{key}")),
                        )
                        .child(
                            text(label)
                                .font(k.ty.text_xsmall)
                                .color(k.c.text_base)
                                .grow(1.)
                                .min_w(0.)
                                .truncate()
                                .help(label)
                                .on_press_tx(move |s: &mut State, tx| {
                                    s.toggle_inventory_source(manager, &text_choice, tx)
                                }),
                        )
                        .child(
                            text(count.to_string())
                                .font(k.ty.text_xsmall)
                                .color(k.c.text_muted)
                                .pr(25.),
                        ),
                );
            }
        }
    }
    let heading = row()
        .w_full()
        .gap(8.)
        .center_y()
        .child(
            text("Sources")
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .grow(1.),
        )
        .child(
            Button::new("All")
                .variant(ButtonVariant::Ghost)
                .on_press_tx(|s: &mut State, tx| {
                    s.inventory.workspace.hidden_providers.clear();
                    s.inventory.workspace.hidden_sources.clear();
                    s.reset_inventory_rows(tx);
                })
                .build(k)
                .h(24.)
                .key("inventory-sources-all"),
        )
        .child(
            Button::new("None")
                .variant(ButtonVariant::Ghost)
                .on_press_tx(|s: &mut State, tx| {
                    s.inventory.workspace.hidden_providers =
                        Manager::ALL.into_iter().map(|m| m.id().into()).collect();
                    s.reset_inventory_rows(tx);
                })
                .build(k)
                .h(24.)
                .key("inventory-sources-none"),
        );
    col()
        .key("inventory-sources")
        .w_full()
        .h_full()
        .min_h(0.)
        .padding(12.)
        .gap(12.)
        .child(heading)
        .child(
            scroll_bar(
                k,
                "inventory-sources-area",
                "scroll",
                rune_kit::popover::fit_scroll(list),
            )
            .w_full()
            .grow(1.)
            .min_h(0.),
        )
        .child(
            row()
                .w_full()
                .pt(10.)
                .border_sides([1., 0., 0., 0.], theme::separator(k))
                .child(
                    text(format!("{} providers scanned", scans.len()))
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_muted),
                ),
        )
}
