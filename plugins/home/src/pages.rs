//! The home screen's pages: each a grid of items in a setting of its own,
//! `home.page-1` to `home.page-8`, their layout as
//! `paperpaw_sdk::widgets::grid` writes it. The first page always
//! shows; the others where they have items. A page added and not filled
//! yet, or emptied, keeps a space ([`widgets::SPACE`], which the grid
//! leaves out) so that it shows until it is removed. Above the grid sits
//! the status line, below it the page dots and the pencil.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{groups, settings};
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets;
use paperpaw_sdk::{
    Canvas, Color, Command, Env, Id, RegisterError, Registrar, SettingExtra, SettingKind,
    SettingSpec, SettingValue,
};

use crate::{ACTIONS, SCREEN};
use paperpaw_sdk::builtin::{actions, menus};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::widgets::grid::{self, Item, What};

/// Pages at most.
pub(crate) const MAX_PAGES: usize = 8;
const SECTION: &str = "home.section";

/// Columns of the grid.
pub(crate) const COLUMNS: Setting<i64> = Setting::new("home.columns");
/// Rows of the grid; 0: as many as make cells about as high as wide.
pub(crate) const ROWS: Setting<i64> = Setting::new("home.rows");
/// Whether the status line shows above the grid.
pub(crate) const STATUS_BAR: Setting<bool> = Setting::new("home.status-bar");
/// Below the page: the pencil that edits it, the dots of the pages, and
/// status items of any plugin.
pub(crate) const FOOTER_PENCIL: Setting<bool> = Setting::new("home.footer.pencil");
pub(crate) const FOOTER_DOTS: Setting<bool> = Setting::new("home.footer.dots");
pub(crate) const FOOTER_ITEMS: Setting<Text> = Setting::new("home.footer.items");

/// Draws the dot of a page (or a layout) `size` pixels wide at `x`, `y`,
/// filled for the current one: round where small shapes are pills or
/// corners round, else square.
pub(crate) fn draw_dot(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    (x, y): (i32, i32),
    size: u32,
    filled: bool,
) {
    let corners = env.settings.text(settings::UI_CORNERS.as_str());
    let round = style.pill() || settings::CORNERS[2..].contains(&corners);
    let radius = if round { size / 2 } else { 0 };
    if filled {
        canvas.fill_round_rect(x, y, size, size, radius, Color::BLACK);
    } else {
        let line = u32::from(style.line()).max(1);
        canvas.stroke_round_rect(x, y, size, size, line, radius, Color::BLACK);
    }
}

/// The built-in widgets (see `blocks`).
pub(crate) const STATUS: &str = "home.status";
pub(crate) const DATE: &str = "home.date";
pub(crate) const CONTINUE: &str = "home.continue";
pub(crate) const RECENT: &str = "home.recent";
/// On a page: every tool not on it otherwise, each in a cell.
pub(crate) const TOOLS: &str = "home.tools";

/// What the pages show at first: the clock, the book being read and the
/// recent ones, then the tools.
pub(crate) const DEFAULTS: [&str; MAX_PAGES] = [
    "home.clock, home.continue, home.recent",
    "home.tools",
    "",
    "",
    "",
    "",
    "",
    "",
];

/// The setting of page `index` (0 the first).
pub(crate) fn setting(index: usize) -> Id {
    Id::parse(&format!("home.page-{}", index + 1)).unwrap_or(Id::new_static("home.page-1"))
}

/// The home screen's part of the Appearance settings.
static IN_HOME: SettingExtra = SettingExtra::new().section(SECTION);
/// A page's items, edited on the home screen itself.
static PAGE: SettingExtra = SettingExtra::new().section(SECTION).editor(SCREEN);
/// The status items below the page, chosen on their picker.
static ITEMS: SettingExtra = SettingExtra::new()
    .section(SECTION)
    .editor(paperpaw_sdk::builtin::screens::STATUS_ITEMS);

/// The setting of page `n` (1 the first).
const fn page(id: &'static str, title: &'static str, default: &'static str) -> SettingSpec {
    SettingSpec::text(id, title, default, groups::LOOK)
        .with_extra(&PAGE)
        .description("home.setting.page.help")
        .advanced()
}

/// The pages, the grid, and what shows above and below it.
static SETTINGS: [SettingSpec; 16] = [
    page("home.page-1", "home.setting.page-1", DEFAULTS[0]),
    page("home.page-2", "home.setting.page-2", DEFAULTS[1]),
    page("home.page-3", "home.setting.page-3", DEFAULTS[2]),
    page("home.page-4", "home.setting.page-4", DEFAULTS[3]),
    page("home.page-5", "home.setting.page-5", DEFAULTS[4]),
    page("home.page-6", "home.setting.page-6", DEFAULTS[5]),
    page("home.page-7", "home.setting.page-7", DEFAULTS[6]),
    page("home.page-8", "home.setting.page-8", DEFAULTS[7]),
    SettingSpec::number(
        COLUMNS.static_str(),
        "home.setting.columns",
        2,
        grid::MAX_COLUMNS as i64,
        3,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .description("home.setting.columns.help"),
    SettingSpec::number(
        ROWS.static_str(),
        "home.setting.rows",
        0,
        grid::MAX_ROWS as i64,
        0,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .description("home.setting.rows.help"),
    SettingSpec::switch(
        crate::blocks::TOOL_LABELS.static_str(),
        "home.setting.tool-labels",
        true,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .description("home.setting.tool-labels.help"),
    SettingSpec::switch(
        crate::blocks::FOCUS_AT_ONCE.static_str(),
        "home.setting.focus-at-once",
        false,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .description("home.setting.focus-at-once.help"),
    SettingSpec::switch(
        STATUS_BAR.static_str(),
        "home.setting.status-bar",
        true,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .description("home.setting.status-bar.help"),
    SettingSpec::switch(
        FOOTER_PENCIL.static_str(),
        "home.setting.footer-pencil",
        true,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .description("home.setting.footer-pencil.help"),
    SettingSpec::switch(
        FOOTER_DOTS.static_str(),
        "home.setting.footer-dots",
        true,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .description("home.setting.footer-dots.help"),
    SettingSpec::new(
        paperpaw_sdk::Id::new_static(FOOTER_ITEMS.static_str()),
        "home.setting.footer-items",
        SettingKind::List,
        SettingValue::Text(Cow::Borrowed("")),
        groups::LOOK,
    )
    .with_extra(&ITEMS)
    .description("home.setting.footer-items.help"),
];

/// The pages as a place of widgets in a grid.
const PLACE: widgets::WidgetPlace = widgets::WidgetPlace::new(
    Id::new_static("home.pages"),
    "home.title",
    PAGE_SETTINGS,
    widgets::PlaceKind::Grid,
);
const PAGE_SETTINGS: &[Id] = &[
    Id::new_static("home.page-1"),
    Id::new_static("home.page-2"),
    Id::new_static("home.page-3"),
    Id::new_static("home.page-4"),
    Id::new_static("home.page-5"),
    Id::new_static("home.page-6"),
    Id::new_static("home.page-7"),
    Id::new_static("home.page-8"),
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&SETTINGS)?;
    r.add_widget_place(PLACE)?;
    r.set_menu_need(
        crate::blocks::FOCUS_AT_ONCE,
        paperpaw_sdk::MenuNeed::NoTouch,
    )?;
    Ok(())
}

/// A page shown: which setting keeps it, and its items.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Page {
    /// The setting it is kept in: 0 for `home.page-1`.
    pub(crate) slot: usize,
    pub(crate) items: Vec<Item>,
}

/// Every page shown, in the order of their settings: the first always,
/// the others where they hold something.
pub(crate) fn pages(env: &Env<'_>) -> Vec<Page> {
    (0..MAX_PAGES)
        .map(|slot| Page {
            slot,
            items: grid::parse(env.settings.text(setting(slot).as_str())),
        })
        .filter(|page| page.slot == 0 || !page.items.is_empty())
        .collect()
}

/// A page's items as the grid shows them: the tools for `home.tools`,
/// without what the grid has no use for (the status line above it,
/// spaces: empty cells are).
pub(crate) fn expand(env: &Env<'_>, items: &[Item]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for item in items {
        match &item.what {
            What::Widget(id) if id == TOOLS => {
                for (action, _) in tools(env) {
                    let shown = items.iter().any(|i| i.what == What::Tool(action.clone()));
                    if !shown {
                        out.push(Item::new(What::Tool(action)));
                    }
                }
            }
            What::Widget(id)
                if id == widgets::SPACE
                    || id == widgets::FILL
                    || (widgets::base(id) == STATUS && env.settings.read(&STATUS_BAR)) => {}
            What::Widget(id) if !env.widgets.has(id) => {}
            _ => out.push(item.clone()),
        }
    }
    out
}

/// Whether the page keeps `item` without showing it ([`expand`] leaves it
/// out): a widget of a plugin not loaded now (switched off, say), the
/// status line while it shows above the grid, a fill.
fn hidden(env: &Env<'_>, item: &Item) -> bool {
    match &item.what {
        What::Widget(id) if id == TOOLS || id == widgets::SPACE => false,
        What::Widget(id) => {
            id == widgets::FILL
                || (widgets::base(id) == STATUS && env.settings.read(&STATUS_BAR))
                || !env.widgets.has(id)
        }
        _ => false,
    }
}

/// The command that keeps `items` (as the page shows them) as the page in
/// `slot`, with what the page keeps there without showing it.
pub(crate) fn store(env: &Env<'_>, slot: usize, items: &[Item]) -> Command {
    let mut items = items.to_vec();
    for item in grid::parse(env.settings.text(setting(slot).as_str())) {
        if hidden(env, &item) && !items.iter().any(|i| i.what == item.what) {
            items.push(item);
        }
    }
    write(slot, &items)
}

/// The command that keeps `items` as the page in `slot` as they are: a
/// page other than the first keeps a space when emptied, so that it still
/// shows.
fn write(slot: usize, items: &[Item]) -> Command {
    let text = if items.is_empty() && slot > 0 {
        String::from(widgets::SPACE)
    } else {
        grid::write(items)
    };
    Command::SetSetting(setting(slot), SettingValue::Text(Cow::Owned(text)))
}

/// The commands that keep `pages` (as [`pages`] reads them, what they do
/// not show included) in the first settings, in their order, and empty
/// the others: once a page was added or removed.
pub(crate) fn store_all(pages: &[Page]) -> Vec<Command> {
    (0..MAX_PAGES)
        .map(|slot| match pages.get(slot) {
            Some(page) => write(slot, &page.items),
            None => Command::SetSetting(setting(slot), SettingValue::Text(Cow::Borrowed(""))),
        })
        .collect()
}

/// The pages fitted to `geometry` once its columns or rows changed: every
/// item where the grid now shows it (moved inside it as little as can
/// be), and what a page has no room for on the next page (a page added
/// where there is none), so that nothing is lost or hidden; `None`
/// where nothing moved. The tools and the spaces stay as they are.
pub(crate) fn reflow(env: &Env<'_>, geometry: &grid::Geometry) -> Option<Vec<Page>> {
    let kept = |item: &Item| match &item.what {
        What::Widget(id) => id == TOOLS || id == widgets::SPACE || id == widgets::FILL,
        _ => false,
    };
    let mut pages = pages(env);
    let mut carried: Vec<Item> = Vec::new();
    let mut changed = false;
    let mut index = 0;
    while index < pages.len() || (!carried.is_empty() && pages.len() < MAX_PAGES) {
        if index == pages.len() {
            pages.push(Page {
                slot: index,
                items: Vec::new(),
            });
        }
        let page = &mut pages[index];
        let (special, mut movable): (Vec<Item>, Vec<Item>) =
            page.items.iter().cloned().partition(|item| kept(item));
        // What did not fit before comes first, so it finds room here.
        movable.splice(0..0, carried.drain(..).map(|item| Item::new(item.what)));
        let (mut placed, left) = grid::fit(env, geometry, &movable);
        placed.extend(special);
        if placed != page.items {
            page.items = placed;
            changed = true;
        }
        carried = left;
        index += 1;
    }
    changed.then_some(pages)
}

/// The setting `id` is, if it keeps a page: 0 for `home.page-1`.
pub(crate) fn slot_of(id: &str) -> Option<usize> {
    (0..MAX_PAGES).find(|&i| setting(i).as_str() == id)
}

/// The tools a page may show: the shortcuts, the tools, what there is to
/// open ([`menus::APPS`], also in the tools' menu), then the settings;
/// their action and the translation key of their name.
pub(crate) fn tools(env: &Env<'_>) -> Vec<(Id, String)> {
    let mut items: Vec<(Id, String)> = Vec::new();
    for menu in [ACTIONS, menus::TOOLS, menus::APPS] {
        for item in env.menus.items(menu.as_str()) {
            // A screenshot from here shows only the home screen: it is
            // for the tools opened over a book (unless listed above).
            let here_useless = menu == menus::TOOLS && *item.id == actions::SCREENSHOT;
            // Hidden by its plugin now (e.g. *Apps* with nothing in it).
            let hidden = !env.menus.state(env, item.id.as_str()).shown;
            if !item.section
                && !here_useless
                && !hidden
                && !items.iter().any(|(id, _)| id == item.id)
            {
                items.push((item.id.clone(), item.title.to_string()));
            }
        }
    }
    if !items.iter().any(|(id, _)| *id == actions::SETTINGS) {
        items.push((actions::SETTINGS, String::from("core.action.settings")));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_stored() {
        let pages = alloc::vec![
            Page {
                slot: 0,
                items: grid::parse("a 0/0 1x1"),
            },
            Page {
                slot: 3,
                items: Vec::new(),
            },
        ];
        let stored = store_all(&pages);
        assert_eq!(stored.len(), MAX_PAGES);
        // In order from the first setting, the empty page kept as a space.
        let text = |n: usize| match &stored[n] {
            Command::SetSetting(id, SettingValue::Text(text)) => {
                (String::from(id.as_str()), String::from(text.as_ref()))
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(text(0), ("home.page-1".into(), "a 0/0 1x1".into()));
        assert_eq!(text(1), ("home.page-2".into(), widgets::SPACE.into()));
        assert_eq!(text(2), ("home.page-3".into(), String::new()));
        assert_eq!(slot_of("home.page-2"), Some(1));
        assert_eq!(slot_of("home.columns"), None);
    }
}
