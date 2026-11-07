use crate::{
    inventory_workspace::{self as workspace, Column},
    model::State,
    theme,
};
use rune::{El, col, icon, row, text};
use rune_kit::{
    Button, ButtonVariant, Checkbox, Dialog, IconButton, IconButtonVariant, Kit, Placement,
    SegmentedControl, glyphs, popover, scrim, scroll_bar,
};

pub fn bar(s: &State, matched: usize, visible_selected: usize, k: &Kit, layers: &mut Vec<El>) -> El {
    let selected = if s.inventory.workspace.selected.is_empty() {
        0
    } else {
        workspace::selected(s).len()
    };
    let hidden = selected.saturating_sub(visible_selected);
    let mut bar = row().w_full().gap(6.).center_y().flex_wrap().child(
        text(if selected == 0 {
            format!("{matched} observations")
        } else if hidden > 0 {
            format!("{selected} selected / {hidden} hidden")
        } else {
            format!("{selected} selected")
        })
        .font(k.ty.text_xsmall)
        .color(k.c.text_muted)
        .grow(1.)
        .min_w(0.),
    );
    if selected > 0 {
        bar = bar.child(
            IconButton::new(&glyphs::CLOSE)
                .variant(IconButtonVariant::Ghost)
                .label("Clear selection")
                .help("Clear selection")
                .on_press_tx(|s: &mut State, tx| {
                    s.inventory.workspace.selected.clear();
                    tx.instant();
                })
                .build(k)
                .size(24., 24.)
                .key("inventory-clear-selection"),
        );
    }
    bar = bar
        .child(
            IconButton::new(&glyphs::COPY)
                .variant(IconButtonVariant::Ghost)
                .label("Copy selected package IDs")
                .help("Copy selected package IDs")
                .disabled(selected == 0)
                .on_press_tx(|s: &mut State, tx| {
                    let selected = workspace::selected(s);
                    let ids = selected
                        .iter()
                        .map(|r| r.package.id.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    tx.copy(ids);
                    s.status = "Selected package IDs copied".into();
                })
                .build(k)
                .size(28., 28.)
                .key("inventory-copy-ids"),
        )
        .child(
            Button::new("Review upgrades")
                .variant(ButtonVariant::Ghost)
                .icon(&crate::sidebar_info::UPDATE)
                .disabled(selected == 0)
                .on_press(|s: &mut State| s.review_inventory_selection())
                .build(k)
                .h(28.)
                .key("inventory-review"),
        )
        .child(
            IconButton::new(&crate::sidebar_info::DOWNLOAD)
                .variant(IconButtonVariant::Ghost)
                .label("Export inventory")
                .help(if selected == 0 {
                    "Export matching observations as JSON"
                } else {
                    "Export all selected observations as JSON, including hidden selections"
                })
                .disabled(
                    s.inventory.workspace.export_busy || (selected == 0 && matched == 0),
                )
                .on_press_tx(|s: &mut State, tx| s.export_inventory(tx))
                .build(k)
                .size(28., 28.)
                .key("inventory-export"),
        )
        .child(
            Button::new("Columns")
                .variant(ButtonVariant::Ghost)
                .icon(&glyphs::FILE_TEXT)
                .expanded(s.inventory.workspace.columns_open)
                .on_press(|s: &mut State| {
                    s.inventory.workspace.columns_open = !s.inventory.workspace.columns_open
                })
                .build(k)
                .h(28.)
                .key("inventory-columns-trigger"),
        );
    if s.inventory.workspace.columns_open {
        let close = |s: &mut State| s.inventory.workspace.columns_open = false;
        let mut content = col()
            .w_full()
            .padding(12.)
            .gap(8.)
            .child(text("Columns").font(k.ty.ui_medium).color(k.c.text_base));
        for column in Column::OPTIONAL {
            content = content.child(
                Checkbox::new(s.inventory.workspace.columns.contains(&column))
                    .label(column.label())
                    .on_toggle_tx(move |s: &mut State, tx| {
                        let w = &mut s.inventory.workspace;
                        if !w.columns.remove(&column) {
                            w.columns.insert(column);
                        } else if w.sort == column {
                            w.sort = Column::Name;
                            w.descending = false;
                        }
                        tx.instant();
                    })
                    .build(k)
                    .key(format!("inventory-column-{column:?}")),
            );
        }
        content = content.child(
            col()
                .w_full()
                .pt(8.)
                .gap(8.)
                .border_sides([1., 0., 0., 0.], theme::separator(k))
                .child(text("Density").font(k.ty.text_xsmall).color(k.c.text_muted))
                .child(
                    SegmentedControl::new(
                        &["Compact", "Comfortable"],
                        Some(usize::from(!s.inventory.workspace.compact)),
                    )
                    .width(None)
                    .label("Inventory row density")
                    .on_pick(|s: &mut State, i, tx| {
                        s.inventory.workspace.compact = i == 0;
                        tx.instant();
                    })
                    .build(k)
                    .key("inventory-density"),
                ),
        );
        let surface = popover::raised(
            k,
            content
                .key("inventory-columns-menu")
                .role("dialog")
                .label("Table columns and density")
                .focus_group()
                .focus_return("inventory-columns-trigger")
                .w(232.),
            6.,
        )
        .fill(k.c.layer_01)
        .border(1., k.c.border_base)
        .modal(true, close);
        layers.push(scrim("inventory-columns-scrim", close));
        layers.push(popover::anchored(
            k,
            surface,
            "inventory-columns-trigger",
            Placement::BottomEnd,
            4.,
        ));
    }
    bar
}

pub fn dialogs(s: &State, k: &Kit, layers: &mut Vec<El>) {
    if let Some(review) = &s.inventory.workspace.review {
        let commands = review
            .items
            .iter()
            .filter_map(|item| item.command.as_deref().ok())
            .collect::<Vec<_>>()
            .join("\n");
        let count = review
            .items
            .iter()
            .filter(|item| item.command.is_ok())
            .count();
        let mut items = col().w_full().gap(0.);
        for (index, item) in review.items.iter().enumerate() {
            items = items.child(
                col()
                    .w_full()
                    .min_w(0.)
                    .gap(6.)
                    .py(10.)
                    .border_sides([0., 0., 1., 0.], theme::separator(k))
                    .child(
                        row()
                            .w_full()
                            .gap(8.)
                            .center_y()
                            .child(crate::provider_badge::badge(item.provider, k))
                            .child(
                                text(&item.name)
                                    .font(k.ty.ui_medium)
                                    .color(k.c.text_base)
                                    .grow(1.)
                                    .min_w(0.)
                                    .truncate(),
                            ),
                    )
                    .child(
                        text(&item.id)
                            .font(k.ty.text_xsmall)
                            .color(k.c.text_muted)
                            .wrap_anywhere(),
                    )
                    .child(
                        text(match &item.command {
                            Ok(command) => command,
                            Err(reason) => reason,
                        })
                        .key(format!("inventory-upgrade-command-{index}"))
                        .font(if item.command.is_ok() {
                            k.ty.text_xsmall.mono()
                        } else {
                            k.ty.text_xsmall
                        })
                        .color(k.c.text_base)
                        .wrap_anywhere()
                        .selectable(true),
                    ),
            );
        }
        let content=col().w_full().padding(16.).gap(12.)
            .child(text(format!("{count} commands / {} observations",review.items.len())).font(k.ty.text_xsmall).color(k.c.text_muted))
            .child(scroll_bar(k,"inventory-review-area","scroll",popover::fit_scroll(items)).w_full().max_h(300.).min_h(0.))
            .child(row().w_full().gap(8.).center_y()
                .child(icon(&glyphs::LOCK,13.).color(k.c.text_muted))
                .child(text("Command preview only. Nothing has run; version availability and compatibility are not verified.")
                    .font(k.ty.text_xsmall).color(k.c.text_muted).grow(1.).min_w(0.).wrap()))
            .child(row().w_full().gap(8.).justify_end()
                .child(Button::new("Close").on_press(|s:&mut State|s.inventory.workspace.review=None).build(k).key("inventory-review-close"))
                .child(Button::new("Copy commands").icon(&glyphs::COPY).disabled(count==0)
                    .on_press_tx(move |s:&mut State,tx|{tx.copy(commands.clone());s.status="Upgrade commands copied; nothing executed".into();})
                    .build(k).key("inventory-review-copy")));
        Dialog::new("inventory-upgrade-review", "Review upgrade commands")
            .open(true)
            .fit()
            .width(660.)
            .on_close(|s: &mut State| s.inventory.workspace.review = None)
            .child(content)
            .build(k, layers);
    }
}
