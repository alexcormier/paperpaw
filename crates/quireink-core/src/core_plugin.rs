//! The kernel's own contributions, registered through the regular plugin
//! interface: standard actions, slots, settings, key bindings and the
//! placeholder screen.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use quireink_sdk::builtin::{
    actions, events, fonts, groups, keymap_presets, menus, params, screens, setting_sections,
    settings, settings_sections, slots, status_items,
};
use quireink_sdk::library;
use quireink_sdk::time::{self, AUTO, CLOCK_12H, CLOCK_24H, TIME_ZONES};
use quireink_sdk::widgets::{self, Drawn, WidgetSpec};
use quireink_sdk::{
    ActionHandler, ActionHint, ActionSpec, Canvas, Color, Command, Cx, Env, Handled, Id, Manifest,
    MenuEntry, Params, Plugin, Priority, RegisterError, Registrar, SDK_VERSION, Screen, ScreenSpec,
    SettingExtra, SettingGroupSpec, SettingKind, SettingSpec, SettingValue, SlotSpec,
    StatusItemSpec, StatusValue,
};

pub(crate) const CORE: Id = Id::new_static("core");

#[derive(Debug)]
pub(crate) struct CorePlugin;

impl Plugin for CorePlugin {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: CORE,
            name: Cow::Borrowed("QuireInk kernel"),
            description: Cow::Borrowed("Runs screens, input, settings and plugins."),
            version: SDK_VERSION,
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        register_screen_actions(r)?;
        register_moving(r)?;
        register_device_actions(r)?;
        register_library(r)?;
        register_screens(r)?;
        register_menus(r)?;
        register_general_settings(r)?;
        register_look_settings(r)?;
        register_device_settings(r)?;
        register_spaces(r)?;
        register_colors(r)?;
        register_reading_settings(r)?;
        register_marks(r)?;
        register_time_and_battery(r)?;
        register_keymaps_and_setup(r)?;
        Ok(())
    }
}

/// The actions screens give a meaning of their own (with their hints in
/// the row), and how a screen takes gestures.
fn register_screen_actions(r: &mut Registrar) -> Result<(), RegisterError> {
    let screen_actions = [
        (
            actions::NEXT_PAGE,
            "core.action.next-page",
            ActionHint::Icon("chevron-down"),
        ),
        (
            actions::PREV_PAGE,
            "core.action.prev-page",
            ActionHint::Icon("chevron-up"),
        ),
        (
            actions::UP,
            "core.action.up",
            ActionHint::Icon("chevron-up"),
        ),
        (
            actions::DOWN,
            "core.action.down",
            ActionHint::Icon("chevron-down"),
        ),
        (
            actions::LEFT,
            "core.action.left",
            ActionHint::Icon("chevron-left"),
        ),
        (
            actions::RIGHT,
            "core.action.right",
            ActionHint::Icon("chevron-right"),
        ),
        (
            actions::CONFIRM,
            "core.action.confirm",
            label("core.hint.core.confirm"),
        ),
    ];
    for (id, title, hint) in screen_actions {
        r.add_action(ActionSpec::new(id, title).hint(hint))?;
    }
    // How a screen takes gestures (`gestures.policy` in key maps): ids
    // to bind, nothing to run.
    for (id, title) in [
        ("core.gestures.standard", "core.gestures.standard"),
        ("core.gestures.own", "core.gestures.own"),
        ("core.gestures.off", "core.gestures.off"),
    ] {
        r.add_action(ActionSpec::new(Id::new_static(id), title))?;
    }
    Ok(())
}

/// The actions screens are moved and left with, and what opens the
/// kernel's slots and menus.
fn register_moving(r: &mut Registrar) -> Result<(), RegisterError> {
    let open = |slot: Id| -> ActionHandler {
        Box::new(move |_: &Env<'_>, out: &mut Vec<Command>| {
            out.push(Command::open_slot(slot.clone()));
        })
    };
    let forward = |target: Id| -> ActionHandler {
        Box::new(move |_: &Env<'_>, out: &mut Vec<Command>| {
            out.push(Command::RunAction(target.clone()));
        })
    };
    let handled: [(Id, &'static str, ActionHandler); 10] = [
        (
            actions::REFRESH_SCREEN,
            "core.action.refresh-screen",
            Box::new(|_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::Redraw(quireink_sdk::RefreshMode::Full));
            }),
        ),
        (
            actions::QUICK_PANEL,
            "core.action.quick-panel",
            Box::new(|_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenSlot(
                    slots::PANEL,
                    Params::new()
                        .with(params::MENU, String::from(menus::QUICK.as_str()))
                        .with(params::MODE, "top"),
                ));
            }),
        ),
        // Screens without their own meaning of next/previous turn pages.
        (
            actions::NEXT,
            "core.action.next",
            forward(actions::NEXT_PAGE),
        ),
        (
            actions::PREVIOUS,
            "core.action.previous",
            forward(actions::PREV_PAGE),
        ),
        (
            actions::MENU,
            "core.action.menu",
            Box::new(|_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenSlot(
                    slots::MENU,
                    Params::new().with(params::MENU, menus::MAIN.as_str()),
                ));
            }),
        ),
        (
            actions::LIBRARY,
            "core.action.library",
            open(slots::LIBRARY),
        ),
        (
            actions::SETTINGS,
            "core.action.settings",
            open(slots::SETTINGS),
        ),
        (
            actions::BACK,
            "core.action.back",
            Box::new(|_: &Env<'_>, out: &mut Vec<Command>| out.push(Command::Back)),
        ),
        (
            actions::NOTHING,
            "core.action.nothing",
            Box::new(|_: &Env<'_>, _: &mut Vec<Command>| {}),
        ),
        (
            actions::HOME,
            "core.action.home",
            Box::new(|_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::open_slot(slots::HOME));
            }),
        ),
    ];
    add_handled(r, handled)
}

/// The actions of the device: cleaning up, screenshots, the tools,
/// sleep and night mode.
fn register_device_actions(r: &mut Registrar) -> Result<(), RegisterError> {
    let handled: [(Id, &'static str, ActionHandler); 5] = [
        (
            actions::CLEAN_UP,
            "core.action.clean-up",
            Box::new(|env: &Env<'_>, out: &mut Vec<Command>| {
                // What is kept for books no longer on the storage.
                let books = library::book_keys(env.storage, env.formats);
                let keep = |key: &str| {
                    u64::from_str_radix(key, 16).is_ok_and(|k| books.binary_search(&k).is_ok())
                };
                let removed = library::clean_up(env.storage, env.book_data, &keep);
                let text = env.i18n.format(
                    "core.cleaned-up",
                    &[("count", quireink_sdk::Arg::Int(i64::from(removed)))],
                );
                out.push(Command::Notify(text));
            }),
        ),
        (
            actions::SCREENSHOT,
            "core.action.screenshot",
            Box::new(|env: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::Screenshot(screenshot_path(env)));
            }),
        ),
        (
            actions::TOOLS,
            "core.action.tools",
            Box::new(|_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenSlot(
                    slots::MENU,
                    Params::new().with(params::MENU, menus::TOOLS.as_str()),
                ));
            }),
        ),
        (
            actions::SLEEP,
            "core.action.sleep",
            Box::new(|env: &Env<'_>, out: &mut Vec<Command>| {
                // The sleep screen first, if there is one, then sleep.
                if env.caps.sleep && env.has_slot(&slots::SLEEP) {
                    out.push(Command::open_slot(slots::SLEEP));
                }
                out.push(Command::Sleep);
            }),
        ),
        (
            actions::TOGGLE_NIGHT_MODE,
            "core.action.toggle-night-mode",
            Box::new(|env: &Env<'_>, out: &mut Vec<Command>| {
                let night = env.settings.read(&settings::NIGHT_MODE);
                out.push(Command::set(&settings::NIGHT_MODE, !night));
            }),
        ),
    ];
    add_handled(r, handled)
}

/// Actions with their handlers, the hint each shows.
fn add_handled<const N: usize>(
    r: &mut Registrar,
    handled: [(Id, &'static str, ActionHandler); N],
) -> Result<(), RegisterError> {
    for (id, title, handler) in handled {
        let hint = match &id {
            id if *id == actions::BACK => label("core.hint.core.back"),
            id if *id == actions::MENU => label("core.hint.core.menu"),
            id if *id == actions::HOME => label("core.hint.core.home"),
            id if *id == actions::NOTHING => ActionHint::Hidden,
            _ => ActionHint::Title,
        };
        r.add_action(ActionSpec::new(id, title).handler(handler).hint(hint))?;
    }
    Ok(())
}

/// The library service and what plugins keep for books, which follow
/// the books.
fn register_library(r: &mut Registrar) -> Result<(), RegisterError> {
    // The library service follows the reading, whichever plugins
    // show it.
    r.on(events::BOOK_OPENED, |env, reading, _| {
        library::record(env, reading, true);
    })?;
    r.on(events::PAGE_TURNED, |env, reading, _| {
        library::record(env, reading, false);
    })?;
    // What plugins keep for a book follows it: moved, removed.
    r.on(events::BOOK_MOVED, |env, moved, _| {
        library::moved(env.storage, &moved.from, &moved.path);
        library::move_data(env.storage, env.book_data, &moved.from, &moved.path);
    })?;
    r.on(events::BOOK_REMOVED, |env, removed, _| {
        library::removed(env.storage, &removed.path);
        library::remove_data(env.storage, env.book_data, &removed.path);
    })?;
    // The notes of books are the kernel's.
    r.add_book_data(quireink_sdk::data_path!("annotations"), "")?;
    r.add_book_data(alloc::format!("{}/books", library::DIR), "")?;
    r.add_book_data(alloc::format!("{}/covers", library::DIR), "")?;
    // In the device's part of the settings, before *About*.
    r.add_menu_entry(quireink_sdk::MenuEntry {
        menu: menus::SETTINGS,
        item: actions::CLEAN_UP,
        order: 98,
    })?;
    r.add_icon(actions::CLEAN_UP, Cow::Borrowed("trash"))?;
    Ok(())
}

/// The kernel's slots, with a placeholder under each, and its own
/// screens.
fn register_screens(r: &mut Registrar) -> Result<(), RegisterError> {
    for (id, title) in [
        (slots::HOME, "core.slot.home"),
        (slots::READER, "core.slot.reader"),
        (slots::LIBRARY, "core.slot.library"),
        (slots::MENU, "core.slot.menu"),
        (slots::SETTINGS, "core.slot.settings"),
    ] {
        r.add_slot(SlotSpec {
            id: id.clone(),
            title: Cow::Borrowed(title),
        })?;
        // Lowest possible priority: any real screen wins.
        r.bind_slot(id, screens::PLACEHOLDER, Priority(i16::MIN))?;
    }
    // Choosing the items or lines of a status line, for any setting.
    r.add_screen(ScreenSpec {
        id: screens::STATUS_ITEMS,
        title: Cow::Borrowed("core.status.choose"),
        factory: Box::new(|env, params| {
            Box::new(quireink_sdk::ui::items::ItemsScreen::open(env, params))
        }),
    })?;
    r.add_screen(ScreenSpec {
        id: screens::STATUS_LINES,
        title: Cow::Borrowed("core.status.choose"),
        factory: Box::new(|env, params| {
            let params = params
                .clone()
                .with(params::MODE, quireink_sdk::ui::items::LINES);
            Box::new(quireink_sdk::ui::items::ItemsScreen::open(env, &params))
        }),
    })?;
    r.add_screen(ScreenSpec {
        id: screens::PLACEHOLDER,
        title: Cow::Borrowed("core.screen.placeholder"),
        factory: Box::new(|_, _| Box::new(PlaceholderScreen)),
    })?;
    Ok(())
}

/// The main menu, the tools, the quick panel and the settings' sections
/// and groups.
fn register_menus(r: &mut Registrar) -> Result<(), RegisterError> {
    // Every reader's submenu "More" (`menus::READER_MORE`): a submenu is
    // an action named like its menu, which the menu shows as a submenu
    // and which opens the menu when bound to a key.
    r.add_action(
        ActionSpec::new(menus::READER_MORE, "core.action.reader-more").handler(Box::new(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenSlot(
                    slots::MENU,
                    Params::new().with(params::MENU, menus::READER_MORE.as_str()),
                ));
            },
        )),
    )?;
    r.add_icon(menus::READER_MORE, Cow::Borrowed("dots"))?;
    // A short main menu; the tools plugins bring are one entry, so it
    // stays the same however many there are.
    r.add_menu_entry(MenuEntry {
        menu: menus::TOOLS,
        item: actions::SCREENSHOT,
        order: 95,
    })?;
    r.add_icon(actions::SCREENSHOT, Cow::Borrowed("camera"))?;
    for (action, order, icon) in [
        (actions::HOME, 10, "home"),
        (actions::LIBRARY, 20, "books"),
        (actions::TOOLS, 30, "apps"),
        (settings::NIGHT_MODE.into(), 85, "moon"),
        (actions::SETTINGS, 90, "settings"),
    ] {
        r.add_menu_entry(MenuEntry {
            menu: menus::MAIN,
            item: action.clone(),
            order,
        })?;
        r.add_icon(action, Cow::Borrowed(icon))?;
    }

    // The quick panel: what is changed often, whatever is open; plugins
    // add their own (the light, …).
    for (item, order) in [
        (settings::NIGHT_MODE.into(), 10),
        (settings::ROTATION.into(), 40),
        (actions::REFRESH_SCREEN, 50),
        (actions::SCREENSHOT, 60),
        (actions::SLEEP, 70),
        (actions::SETTINGS, 90),
    ] {
        r.add_menu_entry(MenuEntry {
            menu: menus::QUICK,
            item,
            order,
        })?;
    }
    r.add_icon(actions::QUICK_PANEL, Cow::Borrowed("adjustments"))?;
    r.add_icon(actions::REFRESH_SCREEN, Cow::Borrowed("refresh"))?;

    // The settings' main page: reading, the device, customising.
    for (id, title, order) in [
        (
            settings_sections::READING,
            "core.settings-section.reading",
            0,
        ),
        (
            settings_sections::DEVICE,
            "core.settings-section.device",
            50,
        ),
        (
            settings_sections::CUSTOMISE,
            "core.settings-section.customise",
            200,
        ),
    ] {
        r.add_menu_section(quireink_sdk::MenuSection {
            id,
            title: Cow::Borrowed(title),
            menu: menus::SETTINGS,
            order,
        })?;
    }
    for (id, title, order, icon) in [
        (groups::READING, "core.group.reading", 10, "book"),
        (groups::LIBRARY, "core.group.library", 20, "books"),
        (groups::STATUS_LINES, "core.group.status", 15, "layout"),
        (groups::MARKS, "core.group.marks", 17, "highlight"),
        (groups::DISPLAY, "core.group.display", 60, "contrast"),
        (groups::CONTROLS, "core.group.controls", 70, "adjustments"),
        (groups::SYSTEM, "core.group.system", 90, "device-tablet"),
        (groups::LOOK, "core.group.look", 210, "palette"),
        (groups::OTHER, "core.group.other", 100, "dots"),
        (groups::DEVELOPER, "core.group.developer", 95, "bug"),
    ] {
        r.add_setting_group(SettingGroupSpec {
            id: id.clone(),
            title: Cow::Borrowed(title),
            order,
            page: None,
        })?;
        r.add_icon(id, Cow::Borrowed(icon))?;
    }
    Ok(())
}

/// Units, steps and sections of the kernel's settings.
static MM: SettingExtra = SettingExtra::new().unit("core.unit.mm");
static PT: SettingExtra = SettingExtra::new().unit("core.unit.pt");
static PAGES: SettingExtra = SettingExtra::new().unit("core.unit.pages");
static SECONDS: SettingExtra = SettingExtra::new().unit("core.unit.seconds");
static MS: SettingExtra = SettingExtra::new().unit("core.unit.ms");
static TENTH_MM: SettingExtra = SettingExtra::new().unit("core.unit.tenth-mm");
static PERCENT_10: SettingExtra = SettingExtra::new().unit("core.unit.percent").step(10);
static PERCENT_5: SettingExtra = SettingExtra::new().unit("core.unit.percent").step(5);
static MINUTES_5: SettingExtra = SettingExtra::new().unit("core.unit.minutes").step(5);
static COLORS: SettingExtra = SettingExtra::new().section("core.section.colors");
static FONT: SettingExtra = SettingExtra::new().section(setting_sections::FONT);
static FONT_PT: SettingExtra = SettingExtra::new()
    .section(setting_sections::FONT)
    .unit("core.unit.pt");

/// The language, night mode, speech and the edges for swipes.
static GENERAL: [SettingSpec; 8] = [
    SettingSpec::fixed_switch(
        settings::DEVELOPER.static_str(),
        "core.setting.developer",
        false,
        groups::SYSTEM,
    )
    .advanced()
    .described("core.setting.developer.help"),
    SettingSpec::fixed(
        settings::LANGUAGE.static_str(),
        "core.setting.language",
        SettingKind::Language,
        SettingValue::Text(Cow::Borrowed(quireink_i18n::FALLBACK_LANGUAGE)),
        groups::SYSTEM,
    )
    .described("core.setting.language.help"),
    SettingSpec::fixed_switch(
        settings::NIGHT_MODE.static_str(),
        "core.setting.night-mode",
        false,
        groups::DISPLAY,
    )
    .described("core.setting.night-mode.help"),
    SettingSpec::fixed_switch(
        settings::NIGHT_PICTURES.static_str(),
        "core.setting.night-pictures",
        false,
        groups::DISPLAY,
    )
    .described("core.setting.night-pictures.help"),
    SettingSpec::fixed_choice(
        settings::MONO_TEXT.static_str(),
        "core.setting.mono-text",
        &settings::MONO_TEXT_LEVELS,
        settings::MONO_TEXT_LEVELS[1],
        groups::DISPLAY,
    )
    .described("core.setting.mono-text.help"),
    SettingSpec::fixed_switch(
        settings::SPEAK_FOCUS.static_str(),
        "core.setting.speak-focus",
        false,
        groups::SYSTEM,
    )
    .described("core.setting.speak-focus.help"),
    SettingSpec::fixed_number(
        settings::EDGE_ENDS.static_str(),
        "core.setting.edge-ends",
        0,
        25,
        10,
        groups::CONTROLS,
    )
    .with_extra(&MM)
    .advanced()
    .described("core.setting.edge.help"),
    SettingSpec::fixed_number(
        settings::EDGE_SIDES.static_str(),
        "core.setting.edge-sides",
        0,
        25,
        5,
        groups::CONTROLS,
    )
    .with_extra(&MM)
    .advanced()
    .described("core.setting.edge.help"),
];

fn register_general_settings(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&GENERAL)?;
    r.set_menu_need(settings::MONO_TEXT, quireink_sdk::MenuNeed::BlackAndWhite)?;
    r.set_menu_need(settings::SPEAK_FOCUS, quireink_sdk::MenuNeed::Speech)?;
    Ok(())
}

/// How the interface looks: its size, header, corners, contrast and
/// room.
static LOOK: [SettingSpec; 7] = [
    SettingSpec::fixed_number(
        settings::UI_SIZE.static_str(),
        "core.setting.ui-size",
        7,
        24,
        9,
        groups::LOOK,
    )
    .with_extra(&PT)
    .described("core.setting.ui-size.help"),
    SettingSpec::fixed_choice(
        settings::UI_HEADER.static_str(),
        "core.setting.ui-header",
        &[settings::HEADER_DARK, settings::HEADER_LIGHT],
        settings::HEADER_LIGHT,
        groups::LOOK,
    )
    .described("core.setting.ui-header.help"),
    SettingSpec::fixed_choice(
        settings::UI_CORNERS.static_str(),
        "core.setting.ui-corners",
        &settings::CORNERS,
        settings::CORNERS[0],
        groups::LOOK,
    )
    .described("core.setting.ui-corners.help"),
    SettingSpec::fixed_choice(
        settings::UI_CONTRAST.static_str(),
        "core.setting.ui-contrast",
        &[settings::CONTRAST_NORMAL, settings::CONTRAST_STRONG],
        settings::CONTRAST_NORMAL,
        groups::LOOK,
    )
    .described("core.setting.ui-contrast.help"),
    SettingSpec::fixed_number(
        settings::UI_MARGIN.static_str(),
        "core.setting.ui-margin",
        0,
        15,
        3,
        groups::LOOK,
    )
    .with_extra(&MM)
    .described("core.setting.ui-margin.help"),
    SettingSpec::fixed_number(
        settings::UI_ROW.static_str(),
        "core.setting.ui-row",
        120,
        300,
        180,
        groups::LOOK,
    )
    .with_extra(&PERCENT_10)
    .described("core.setting.ui-row.help"),
    SettingSpec::fixed_number(
        settings::UI_LINES.static_str(),
        "core.setting.ui-lines",
        1,
        10,
        3,
        groups::LOOK,
    )
    .with_extra(&TENTH_MM)
    .described("core.setting.ui-lines.help"),
];

fn register_look_settings(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&LOOK)
}

/// Buttons, the panel's refresh, sleep, rotation, notices and long
/// presses.
static DEVICE: [SettingSpec; 8] = [
    SettingSpec::fixed_choice(
        settings::KEYMAP_PRESET.static_str(),
        "core.setting.keymap-preset",
        &[keymap_presets::STANDARD_NAME],
        keymap_presets::STANDARD_NAME,
        groups::CONTROLS,
    )
    .described("core.setting.keymap-preset.help"),
    SettingSpec::fixed_number(
        settings::FULL_REFRESH_INTERVAL.static_str(),
        "core.setting.full-refresh-interval",
        0,
        1000,
        6,
        groups::DISPLAY,
    )
    .advanced()
    .with_extra(&PAGES)
    .described("core.setting.full-refresh-interval.help"),
    SettingSpec::fixed_switch(
        settings::FLASH_SCREENS.static_str(),
        "core.setting.flash-screens",
        true,
        groups::DISPLAY,
    )
    .advanced()
    .described("core.setting.flash-screens.help"),
    SettingSpec::fixed_number(
        settings::SLEEP_AFTER.static_str(),
        "core.setting.sleep-after",
        0,
        120,
        10,
        groups::SYSTEM,
    )
    .with_extra(&MINUTES_5)
    .described("core.setting.sleep-after.help"),
    SettingSpec::fixed_choice(
        settings::ROTATION.static_str(),
        "core.setting.rotation",
        &settings::ROTATIONS,
        settings::ROTATIONS[0],
        groups::DISPLAY,
    )
    .described("core.setting.rotation.help"),
    SettingSpec::fixed_switch(
        settings::BUTTON_HINTS.static_str(),
        "core.setting.button-hints",
        true,
        groups::CONTROLS,
    )
    .described("core.setting.button-hints.help"),
    SettingSpec::fixed_number(
        settings::NOTICE_SECONDS.static_str(),
        "core.setting.notice-seconds",
        1,
        60,
        4,
        groups::DISPLAY,
    )
    .advanced()
    .with_extra(&SECONDS)
    .described("core.setting.notice-seconds.help"),
    SettingSpec::fixed_number(
        settings::LONG_PRESS_MS.static_str(),
        "core.setting.long-press-ms",
        200,
        3000,
        500,
        groups::CONTROLS,
    )
    .advanced()
    .with_extra(&MS)
    .described("core.setting.long-press-ms.help"),
];

fn register_device_settings(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&DEVICE)?;
    r.set_menu_need(settings::SLEEP_AFTER, quireink_sdk::MenuNeed::Sleep)?;
    r.set_menu_need(settings::ROTATION, quireink_sdk::MenuNeed::Rotation)?;
    r.set_menu_need(settings::BUTTON_HINTS, quireink_sdk::MenuNeed::ButtonRow)?;
    Ok(())
}

/// The colours of text and paper, each with its own colour code.
static COLOR_SETTINGS: [SettingSpec; 4] = [
    SettingSpec::fixed_choice(
        settings::TEXT_COLOR.static_str(),
        "core.setting.text-color",
        &settings::TEXT_COLORS,
        settings::TEXT_COLORS[0],
        groups::LOOK,
    )
    .with_extra(&COLORS)
    .described("core.setting.colors.help"),
    SettingSpec::fixed_text(
        settings::TEXT_COLOR_OWN.static_str(),
        "core.setting.text-color-own",
        "#5b4636",
        groups::LOOK,
    )
    .with_extra(&COLORS)
    .described("core.setting.color-own.help"),
    SettingSpec::fixed_choice(
        settings::PAPER_COLOR.static_str(),
        "core.setting.paper-color",
        &settings::PAPER_COLORS,
        settings::PAPER_COLORS[0],
        groups::LOOK,
    )
    .with_extra(&COLORS)
    .described("core.setting.colors.help"),
    SettingSpec::fixed_text(
        settings::PAPER_COLOR_OWN.static_str(),
        "core.setting.paper-color-own",
        "#f4ecd8",
        groups::LOOK,
    )
    .with_extra(&COLORS)
    .described("core.setting.color-own.help"),
];

fn register_colors(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&COLOR_SETTINGS)?;
    for id in [
        settings::TEXT_COLOR,
        settings::TEXT_COLOR_OWN,
        settings::PAPER_COLOR,
        settings::PAPER_COLOR_OWN,
    ] {
        r.set_menu_need(id, quireink_sdk::MenuNeed::Colors)?;
    }
    Ok(())
}

/// The reading font, and whether a book's settings are its own. The
/// status lines' font: the menus', the book's or one of its own (the
/// families are offered once all fonts are known).
static READING: [SettingSpec; 4] = [
    SettingSpec::fixed_choice(
        settings::STATUS_FONT.static_str(),
        "core.setting.status-font",
        &[settings::STATUS_FONT_UI, settings::STATUS_FONT_BOOK],
        settings::STATUS_FONT_UI,
        groups::STATUS_LINES,
    )
    .described("core.setting.status-font.help"),
    SettingSpec::fixed_choice(
        settings::READING_FONT.static_str(),
        "core.setting.reading-font",
        &[settings::DEFAULT_FONT],
        settings::DEFAULT_FONT,
        groups::READING,
    )
    .with_extra(&FONT)
    .scoped()
    .described("core.setting.reading-font.help"),
    SettingSpec::fixed_number(
        settings::FONT_SIZE.static_str(),
        "core.setting.font-size",
        6,
        40,
        11,
        groups::READING,
    )
    .with_extra(&FONT_PT)
    .scoped()
    .described("core.setting.font-size.help"),
    SettingSpec::fixed_switch(
        settings::SCOPE_ONLY.static_str(),
        "core.setting.scope-only",
        false,
        groups::READING,
    )
    .scoped()
    .described("core.setting.scope-only.help"),
];

fn register_reading_settings(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&READING)
}

/// The roles of the marks every reader draws: highlights, links, a
/// search hit, the sentence read aloud (see `quireink_sdk::decorations`).
mod marks {
    use quireink_sdk::SettingSpec;
    use quireink_sdk::decorations::{Decoration, LineKind, Tone};
    use quireink_sdk::role_settings;

    pub(super) static HIGHLIGHT: [SettingSpec; 7] = role_settings!(
        "core.highlight",
        "core.role.highlight",
        Decoration::NONE.background(Tone::LightGray)
    );
    pub(super) static LINK: [SettingSpec; 7] = role_settings!(
        "core.link",
        "core.role.link",
        Decoration::NONE
            .line(LineKind::Solid)
            .line_tone(Tone::DarkGray)
            .thickness(1)
    );
    pub(super) static SEARCH_HIT: [SettingSpec; 7] = role_settings!(
        "core.search-hit",
        "core.role.search-hit",
        Decoration::NONE.line(LineKind::Solid).thickness(5)
    );
    pub(super) static ALOUD: [SettingSpec; 7] = role_settings!(
        "core.aloud",
        "core.role.aloud",
        Decoration::NONE.line(LineKind::Solid).thickness(5)
    );
}

fn register_marks(r: &mut Registrar) -> Result<(), RegisterError> {
    for role in [
        &marks::HIGHLIGHT,
        &marks::LINK,
        &marks::SEARCH_HIT,
        &marks::ALOUD,
    ] {
        r.add_settings(role)?;
    }
    Ok(())
}

/// The time zones to choose from: the system's, then each known one.
const ZONES: [&str; TIME_ZONES.len() + 1] = {
    let mut zones = [AUTO; TIME_ZONES.len() + 1];
    let mut i = 0;
    while i < TIME_ZONES.len() {
        zones[i + 1] = TIME_ZONES[i].0;
        i += 1;
    }
    zones
};

/// How the time is shown, and where it is.
static TIME: [SettingSpec; 4] = [
    SettingSpec::fixed_choice(
        settings::CLOCK_FORMAT.static_str(),
        "core.setting.clock-format",
        &[CLOCK_24H, CLOCK_12H],
        CLOCK_24H,
        groups::SYSTEM,
    ),
    SettingSpec::fixed_choice(
        settings::WEEK_START.static_str(),
        "core.setting.week-start",
        &time::WEEK_STARTS,
        time::WEEK_STARTS[0],
        groups::SYSTEM,
    ),
    SettingSpec::fixed_choice(
        settings::TIME_ZONE.static_str(),
        "core.setting.time-zone",
        &ZONES,
        AUTO,
        groups::SYSTEM,
    )
    .described("core.setting.time-zone.help"),
    SettingSpec::fixed_text(
        settings::TIME_ZONE_RULE.static_str(),
        "core.setting.time-zone-rule",
        "",
        groups::SYSTEM,
    )
    .advanced()
    .described("core.setting.time-zone-rule.help"),
];

/// Below which charge the battery shows when it shows only when low.
static BATTERY_LOW: [SettingSpec; 1] = [SettingSpec::fixed_number(
    settings::BATTERY_LOW.static_str(),
    "core.setting.battery-low",
    5,
    50,
    20,
    groups::SYSTEM,
)
.with_extra(&PERCENT_5)
.advanced()
.described("core.setting.battery-low.help")];

/// The time of day and the battery, for status lines.
fn register_time_and_battery(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&TIME)?;
    r.add_status_item(StatusItemSpec {
        id: status_items::CLOCK,
        title: Cow::Borrowed("core.status.clock"),
        value: Box::new(|env, _| time::clock_text(env).map(StatusValue::Text)),
    })?;
    r.add_status_item(StatusItemSpec {
        id: status_items::DATE,
        title: Cow::Borrowed("core.status.date"),
        value: Box::new(|env, _| time::date_text(env).map(StatusValue::Text)),
    })?;
    r.add_status_item(StatusItemSpec {
        id: status_items::BATTERY,
        title: Cow::Borrowed("core.status.battery"),
        value: Box::new(|env, _| {
            env.status.battery.map(|b| StatusValue::Level {
                percent: b.percent,
                charging: b.charging,
            })
        }),
    })?;
    r.add_status_item(StatusItemSpec {
        id: status_items::BATTERY_LOW,
        title: Cow::Borrowed("core.status.battery-low"),
        value: Box::new(|env, _| {
            let low = env.settings.read(&settings::BATTERY_LOW);
            env.status
                .battery
                .filter(|b| i64::from(b.percent) <= low)
                .map(|b| StatusValue::Level {
                    percent: b.percent,
                    charging: b.charging,
                })
        }),
    })?;
    r.add_settings(&BATTERY_LOW)?;
    Ok(())
}

/// The key maps and the first-run setup, and the kernel's words.
fn register_keymaps_and_setup(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_keymap(Cow::Borrowed(include_str!("../keymaps/default.ini")))?;
    r.add_keymap_preset(
        keymap_presets::TWO_BUTTONS,
        Cow::Borrowed(include_str!("../keymaps/two-buttons.ini")),
    )?;
    r.add_keymap_preset(
        keymap_presets::LEFT_HANDED,
        Cow::Borrowed(include_str!("../keymaps/left-handed.ini")),
    )?;
    // The first-run setup: the language, the buttons where there is
    // no touch, the time zone where the system has none.
    r.add_setup_step(quireink_sdk::setup::SetupStepSpec {
        id: Id::new_static("core.setup.language"),
        title: Cow::Borrowed(""),
        help: Cow::Borrowed(""),
        order: 10,
        page: quireink_sdk::setup::SetupPage::Setting(settings::LANGUAGE.into()),
        applies: None,
    })?;
    r.add_setup_step(quireink_sdk::setup::SetupStepSpec {
        id: Id::new_static("core.setup.buttons"),
        title: Cow::Borrowed(""),
        help: Cow::Borrowed(""),
        order: 20,
        page: quireink_sdk::setup::SetupPage::Setting(settings::KEYMAP_PRESET.into()),
        applies: Some(Box::new(|env: &quireink_sdk::Env<'_>| {
            env.device.input.buttons && !env.device.input.touch
        })),
    })?;
    r.add_setup_step(quireink_sdk::setup::SetupStepSpec {
        id: Id::new_static("core.setup.time-zone"),
        title: Cow::Borrowed(""),
        help: Cow::Borrowed("core.setup.time-zone.help"),
        order: 40,
        page: quireink_sdk::setup::SetupPage::Setting(settings::TIME_ZONE.into()),
        applies: Some(Box::new(|env: &quireink_sdk::Env<'_>| {
            env.status
                .clock
                .is_none_or(|c| c.utc_offset_minutes.is_none())
        })),
    })?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
    Ok(())
}

/// Shown when no plugin provides a screen for a slot.
#[derive(Debug)]
struct PlaceholderScreen;

impl Screen for PlaceholderScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let area = canvas.clip();
        let margin = env.metrics.mm(4);
        let line = env.metrics.tenth_mm(5);
        let (w, h) = (u32::from(area.width), u32::from(area.height));
        let inner_w = w.saturating_sub(2 * margin);
        let inner_h = h.saturating_sub(2 * margin);
        canvas.stroke_rect(
            margin as i32,
            margin as i32,
            inner_w,
            inner_h,
            line,
            Color::BLACK,
        );
        if let Some(font) = env.fonts.role(fonts::UI.as_str()) {
            let size = env.metrics.pt(14.0);
            let text = env.i18n.tr("core.screen.placeholder");
            let width = font.measure(size, text);
            let x = (w as f32 - width) / 2.0;
            let baseline = (h / 2) as i32 + (size / 3.0) as i32;
            font.draw(canvas, size, x, baseline, text, Color::BLACK);
        }
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// Where a screenshot goes: `Screenshots` beside the library folder,
/// named by the time ("QuireInk 2026-10-15 17-30-05.png").
fn screenshot_path(env: &Env<'_>) -> alloc::string::String {
    let library = quireink_sdk::library::folder(env);
    let parent = quireink_sdk::files::parent(library);
    let dir = if parent.is_empty() {
        alloc::string::String::from("Screenshots")
    } else {
        alloc::format!("{parent}/Screenshots")
    };
    let _ = env.storage.create_dir(&dir);
    let name = match quireink_sdk::time::local_time(env) {
        Some(t) => alloc::format!(
            "QuireInk {}-{:02}-{:02} {:02}-{:02}-{:02}",
            t.year,
            t.month,
            t.day,
            t.hour,
            t.minute,
            t.second
        ),
        None => alloc::format!("QuireInk {}", env.now.0),
    };
    let mut path = alloc::format!("{dir}/{name}.png");
    let mut n = 2;
    while env.storage.exists(&path) {
        path = alloc::format!("{dir}/{name} ({n}).png");
        n += 1;
    }
    path
}

/// The size of the gap widget.
static SPACE: [SettingSpec; 1] = [SettingSpec::fixed_choice(
    widgets::SPACE_SIZE.static_str(),
    "core.setting.space-size",
    &widgets::SPACE_SIZES,
    widgets::SPACE_SIZES[1],
    Id::new_static(widgets::SPACE),
)
.described("core.setting.space-size.help")];

/// The widgets for room between the others: a gap of a set height, and
/// the room left (see `quireink_sdk::widgets`).
fn register_spaces(r: &mut Registrar) -> Result<(), RegisterError> {
    let space = Id::new_static(widgets::SPACE);
    r.add_setting_group(widgets::settings_group(&space, "core.widget.space"))?;
    r.add_settings(&SPACE)?;
    let space_height = |env: &Env<'_>, _| {
        let size = env.settings.read(&widgets::SPACE_SIZE);
        let mm = match widgets::SPACE_SIZES.iter().position(|s| *s == size) {
            Some(0) => 4,
            Some(2) => 20,
            _ => 10,
        };
        env.metrics.mm(mm).min(u32::from(u16::MAX)) as u16
    };
    r.add_widget(
        WidgetSpec::drawn(
            space,
            "core.widget.space",
            Drawn::new(space_height, |_, _, _| {}),
        )
        .category(widgets::categories::LAYOUT),
    )?;
    // Screens that place widgets give it the room left.
    r.add_widget(
        WidgetSpec::drawn(
            Id::new_static(widgets::FILL),
            "core.widget.fill",
            Drawn::new(|_, _| 0, |_, _, _| {}),
        )
        .category(widgets::categories::LAYOUT),
    )?;
    Ok(())
}

/// A hint that is a word of its own (a translation key).
fn label(key: &'static str) -> ActionHint {
    ActionHint::Label(Cow::Borrowed(key))
}

#[cfg(test)]
mod tests {
    use quireink_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
