use crate::{
    dropdown,
    model::{PackageRow, State},
    sidebar_actions,
};
use rune::{El, Icon, TextStyle, col, row, text};
use rune_kit::{Button, Dialog, Kit, glyphs};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Intent {
    Latest,
    Last20,
    Installed,
    Breaking,
    Description,
    Name,
}
impl Intent {
    fn icon(self) -> &'static Icon {
        match self {
            Self::Latest => &Icon {
                name: "menu-latest",
                view: 24.,
                body: include_str!("../assets/status/file-text.svg"),
            },
            Self::Last20 => &Icon {
                name: "menu-history",
                view: 24.,
                body: include_str!("../assets/status/history.svg"),
            },
            Self::Installed => &Icon {
                name: "menu-compare",
                view: 24.,
                body: include_str!("../assets/status/git-compare-arrows.svg"),
            },
            Self::Breaking => &Icon {
                name: "menu-breaking",
                view: 24.,
                body: include_str!("../assets/status/triangle-alert.svg"),
            },
            Self::Description => &Icon {
                name: "menu-description",
                view: 24.,
                body: include_str!("../assets/status/pencil-line.svg"),
            },
            Self::Name => &Icon {
                name: "menu-name",
                view: 24.,
                body: include_str!("../assets/status/case-sensitive.svg"),
            },
        }
    }
    pub const ALL: [Self; 6] = [
        Self::Latest,
        Self::Last20,
        Self::Installed,
        Self::Breaking,
        Self::Description,
        Self::Name,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Latest => "Brief latest update",
            Self::Last20 => "Brief last 20 updates",
            Self::Installed => "Summarize since installed",
            Self::Breaking => "Explain breaking changes",
            Self::Description => "Update description",
            Self::Name => "Update name",
        }
    }
}
static COPY: Icon = Icon {
    name: "menu-copy",
    view: 24.,
    body: include_str!("../assets/status/copy.svg"),
};
pub struct Open {
    pub row: String,
    pub package: String,
    pub key: String,
}
pub struct Preview {
    pub name: String,
    pub intent: Intent,
}
#[derive(Default)]
pub struct Ui {
    pub open: Option<Open>,
    pub preview: Option<Preview>,
}

pub fn key(r: &PackageRow) -> String {
    format!("sidebar-ai-{}", crate::activity::hash(&r.id))
}
pub fn button(s: &State, r: &PackageRow, k: &Kit) -> El {
    let menu = key(r);
    let trigger = format!("{menu}-trigger");
    let package = s.catalog.packages[r.package].id.clone();
    let row = r.id.clone();
    sidebar_actions::action(
        trigger.clone(),
        &glyphs::SPARKLE,
        &format!("AI actions for {}", r.name),
        k,
    )
    .expanded(s.ai.open.as_ref().is_some_and(|o| o.key == menu))
    .on_press_tx(move |s: &mut State, tx| {
        tx.focus(&trigger, false);
        s.ai.open = Some(Open {
            row: row.clone(),
            package: package.clone(),
            key: menu.clone(),
        });
    })
}

fn close(s: &mut State) {
    s.ai.open = None;
}

pub fn scene(s: &mut State) {
    crate::sidebar_actions::scene(s, crate::sidebar_actions::Mode::Details);
    s.sidebar_actions.panel = None;
    let r = s
        .rows
        .iter()
        .find(|r| Some(r.id.as_str()) == s.table.cursor())
        .unwrap();
    s.ai.open = Some(Open {
        row: r.id.clone(),
        package: s.catalog.packages[r.package].id.clone(),
        key: key(r),
    });
}
pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>) {
    if let Some(open) = &s.ai.open
        && let Some(r) = crate::sidebar_groups::shown(s)
            .iter()
            .map(|&i| &s.rows[i])
            .find(|r| r.id == open.row)
        && let Some(p) = s.catalog.packages.iter().find(|p| p.id == open.package)
    {
        let header = row()
            .key("ai-menu-title")
            .px(8.)
            .py(6.)
            .gap(8.)
            .center_y()
            .child(
                crate::ui::mark(&s.catalog.packages[r.package], k, 16.).key("ai-menu-title-icon"),
            )
            .child(
                text(&p.name)
                    .key("ai-menu-title-label")
                    .font(TextStyle::new(14., 500.).lh(20.))
                    .color(k.c.text_muted)
                    .min_w(0.)
                    .grow(1.)
                    .truncate()
                    .help_when_truncated(&p.name),
            );
        let mut items = col().items_stretch();
        for (index, intent) in Intent::ALL.into_iter().enumerate() {
            if intent == Intent::Description {
                items = items.child(dropdown::separator("ai-menu-edit-separator", k));
            }
            let name = p.name.clone();
            items = items.child(
                dropdown::item(
                    format!("{}-item{index}", open.key),
                    intent.label(),
                    intent.icon(),
                    k,
                )
                .on_press(move |s: &mut State| {
                    s.ai.preview = Some(Preview {
                        name: name.clone(),
                        intent,
                    });
                    s.ai.open = None;
                    s.sidebar_actions.panel = None;
                }),
            );
        }
        let id = p.id.clone();
        items = items
            .child(dropdown::separator("ai-menu-copy-separator", k))
            .child(
                dropdown::item(format!("{}-copy", open.key), "Copy AI context", &COPY, k).on_press(
                    move |s: &mut State| {
                        s.ai.open = None;
                        s.prepare_brief(&id);
                    },
                ),
            );
        dropdown::with_header(&open.key, 256., header, items, k, layers, close);
    }
    if let Some(preview) = &s.ai.preview {
        Dialog::new("ai-action-preview", preview.intent.label())
            .open(true)
            .fit()
            .width(420.)
            .on_close(|s: &mut State| s.ai.preview = None)
            .child(
                col()
                    .w_full()
                    .padding(16.)
                    .gap(14.)
                    .child(
                        text(&preview.name)
                            .font(k.ty.ui_medium)
                            .color(k.c.text_base)
                            .wrap(),
                    )
                    .child(
                        text("AI not connected")
                            .font(k.ty.text_small)
                            .color(k.c.text_muted),
                    )
                    .child(
                        Button::new("Close")
                            .on_press(|s: &mut State| s.ai.preview = None)
                            .build(k)
                            .key("ai-action-close"),
                    ),
            )
            .build(k, layers);
    }
}
