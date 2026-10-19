//! The home screen's pages: each a grid of items in a setting of its own,
//! `home.page-1` to `home.page-4`, their layout as
//! `paperpaw_sdk::widgets::grid` writes it. The first page always
//! shows; the others where they have items. Above the grid sits the
//! status line, below it the page dots and the pencil.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::widgets;
use paperpaw_sdk::{
    Command, Env, Id, RegisterError, Registrar, SettingExtra, SettingKind, SettingSpec,
    SettingValue,
};

use crate::{ACTIONS, SCREEN};
use paperpaw_sdk::builtin::{actions, menus};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::widgets::grid::{self, Item, What};

/// Pages at most.
pub(crate) const MAX_PAGES: usize = 4;
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
    SettingSpec::fixed_text(id, title, default, groups::LOOK)
        .with_extra(&PAGE)
        .described("home.setting.page.help")
        .advanced()
}

/// The pages, the grid, and what shows above and below it.
static SETTINGS: [SettingSpec; 12] = [
    page("home.page-1", "home.setting.page-1", DEFAULTS[0]),
    page("home.page-2", "home.setting.page-2", DEFAULTS[1]),
    page("home.page-3", "home.setting.page-3", DEFAULTS[2]),
    page("home.page-4", "home.setting.page-4", DEFAULTS[3]),
    SettingSpec::fixed_number(
        COLUMNS.static_str(),
        "home.setting.columns",
        2,
        grid::MAX_COLUMNS as i64,
        4,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .described("home.setting.columns.help"),
    SettingSpec::fixed_number(
        ROWS.static_str(),
        "home.setting.rows",
        0,
        grid::MAX_ROWS as i64,
        0,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .described("home.setting.rows.help"),
    SettingSpec::fixed_switch(
        crate::blocks::TOOL_LABELS.static_str(),
        "home.setting.tool-labels",
        true,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .described("home.setting.tool-labels.help"),
    SettingSpec::fixed_switch(
        crate::blocks::FOCUS_AT_ONCE.static_str(),
        "home.setting.focus-at-once",
        false,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .described("home.setting.focus-at-once.help"),
    SettingSpec::fixed_switch(
        STATUS_BAR.static_str(),
        "home.setting.status-bar",
        true,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .described("home.setting.status-bar.help"),
    SettingSpec::fixed_switch(
        FOOTER_PENCIL.static_str(),
        "home.setting.footer-pencil",
        true,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .described("home.setting.footer-pencil.help"),
    SettingSpec::fixed_switch(
        FOOTER_DOTS.static_str(),
        "home.setting.footer-dots",
        true,
        groups::LOOK,
    )
    .with_extra(&IN_HOME)
    .described("home.setting.footer-dots.help"),
    SettingSpec::fixed(
        FOOTER_ITEMS.static_str(),
        "home.setting.footer-items",
        SettingKind::List,
        SettingValue::Text(Cow::Borrowed("")),
        groups::LOOK,
    )
    .with_extra(&ITEMS)
    .described("home.setting.footer-items.help"),
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&SETTINGS)?;
    r.set_menu_need(
        crate::blocks::FOCUS_AT_ONCE,
        paperpaw_sdk::MenuNeed::NoTouch,
    )?;
    Ok(())
}

/// The items of every page shown, the first page always.
pub(crate) fn pages(env: &Env<'_>) -> Vec<Vec<Item>> {
    let mut pages: Vec<Vec<Item>> = (0..MAX_PAGES)
        .map(|index| grid::parse(env.settings.text(setting(index).as_str())))
        .enumerate()
        .filter(|(index, page)| *index == 0 || !page.is_empty())
        .map(|(_, page)| page)
        .collect();
    if pages.is_empty() {
        pages.push(Vec::new());
    }
    pages
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

/// The commands that store `pages` (the rest emptied).
pub(crate) fn store(pages: &[Vec<Item>]) -> Vec<Command> {
    (0..MAX_PAGES)
        .map(|index| {
            let text = pages.get(index).map(|p| grid::write(p)).unwrap_or_default();
            Command::SetSetting(setting(index), SettingValue::Text(Cow::Owned(text)))
        })
        .collect()
}

/// The page whose setting is `id`, if it is one.
pub(crate) fn page_of(id: &str) -> Option<usize> {
    (0..MAX_PAGES).find(|&i| setting(i).as_str() == id)
}

/// The tools a page may show: the shortcuts, the tools, then the
/// settings; their action and the translation key of their name.
pub(crate) fn tools(env: &Env<'_>) -> Vec<(Id, String)> {
    let mut items: Vec<(Id, String)> = Vec::new();
    for menu in [ACTIONS, menus::TOOLS] {
        for item in env.menus.items(menu.as_str()) {
            // A screenshot from here shows only the home screen: it is
            // for the tools opened over a book (unless listed above).
            let here_useless = menu == menus::TOOLS && *item.id == actions::SCREENSHOT;
            if !item.section && !here_useless && !items.iter().any(|(id, _)| id == item.id) {
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
        let pages = alloc::vec![grid::parse("a 0/0 1x1")];
        assert_eq!(store(&pages).len(), MAX_PAGES);
        assert_eq!(page_of("home.page-2"), Some(1));
        assert_eq!(page_of("home.columns"), None);
    }
}
