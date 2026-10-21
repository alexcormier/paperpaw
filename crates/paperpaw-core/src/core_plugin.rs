//! The kernel's own contributions, registered through the regular plugin
//! interface: standard actions, slots, settings, key bindings and the
//! placeholder screen.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{
    actions, events, fonts, groups, keymap_presets, menus, params, screens, setting_sections,
    settings, settings_sections, slots, status_items,
};
use paperpaw_sdk::library;
use paperpaw_sdk::time::{self, AUTO, CLOCK_12H, CLOCK_24H, TIME_ZONES};
use paperpaw_sdk::widgets::{self, Drawn, WidgetSpec};
use paperpaw_sdk::{
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
        Manifest::new(CORE, "PaperPaw kernel", SDK_VERSION)
            .description("Runs screens, input, settings and plugins.")
            .uses(paperpaw_sdk::Uses::NOTHING)
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
        r.add_presets(&POWER_MODES)?;
        // Text typed for a setting (`ui::setting_opens`), whichever screen
        // asked.
        r.on(events::SETTING_TYPED, |env, typed, out| {
            let id = typed
                .context
                .get(params::ID)
                .and_then(|id| Id::parse(id).ok());
            if let Some(id) = id.filter(|id| env.settings.spec(id.as_str()).is_some()) {
                let text = String::from(typed.text.trim());
                out.push(Command::SetSetting(
                    id,
                    paperpaw_sdk::SettingValue::Text(Cow::Owned(text)),
                ));
            }
        })?;
        // Each plugin's network (its settings made by the kernel).
        r.add_setting_group(
            SettingGroupSpec::new(crate::work::NETWORK_GROUP, "core.group.network").order(87),
        )?;
        r.add_icon(crate::work::NETWORK_GROUP, Cow::Borrowed("world"))?;
        Ok(())
    }
}

/// The power modes built in (ADR 0025): a level, how much longer
/// declared work may wait, and values worn over the reader's own (a
/// number after `<=` is a limit).
static POWER_MODES: [paperpaw_sdk::presets::Preset; 3] = [
    paperpaw_sdk::presets::Preset::fixed(
        "core.normal",
        "core.power-mode",
        "core.power.normal",
        "core.power.normal.help",
        "power.level = normal",
    ),
    paperpaw_sdk::presets::Preset::fixed(
        "core.saving",
        "core.power-mode",
        "core.power.saving",
        "core.power.saving.help",
        "power.level = saving\npower.stretch = 4\ncore.sleep-after <= 5\nx4.frontlight <= 50",
    ),
    paperpaw_sdk::presets::Preset::fixed(
        "core.offline",
        "core.power-mode",
        "core.power.offline",
        "core.power.offline.help",
        "power.level = offline\npower.stretch = 4\ncore.sleep-after <= 5\nx4.frontlight <= 30",
    ),
];

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
                out.push(Command::Redraw(paperpaw_sdk::RefreshMode::Full));
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
                // What is kept for files no longer on the storage: of any
                // format, by any spelling; nothing where a folder could not
                // be read.
                let library = library::folder(env);
                let text = match library::clean_up_unused(env.storage, env.book_data, library) {
                    Ok(removed) => env.i18n.format(
                        "core.cleaned-up",
                        &[("count", paperpaw_sdk::Arg::Int(i64::from(removed)))],
                    ),
                    Err(_) => String::from(env.i18n.tr("core.clean-up-failed")),
                };
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
    r.add_book_data(paperpaw_sdk::data_path!("annotations"), "")?;
    r.add_book_data(alloc::format!("{}/books", library::DIR), "")?;
    r.add_book_data(alloc::format!("{}/covers", library::DIR), "")?;
    // In the device's part of the settings, before *About*.
    r.add_menu_entry(paperpaw_sdk::MenuEntry::new(menus::SETTINGS, actions::CLEAN_UP).order(98))?;
    r.add_icon(actions::CLEAN_UP, Cow::Borrowed("eraser"))?;
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
        r.add_slot(SlotSpec::new(id.clone(), title))?;
        // Lowest possible priority: any real screen wins.
        r.bind_slot(id, screens::PLACEHOLDER, Priority(i16::MIN))?;
    }
    // Choosing the items or lines of a status line, for any setting.
    r.add_screen(ScreenSpec::new(
        screens::STATUS_ITEMS,
        "core.status.choose",
        Box::new(|env, params| Box::new(paperpaw_sdk::ui::items::ItemsScreen::open(env, params))),
    ))?;
    r.add_screen(ScreenSpec::new(
        screens::STATUS_LINES,
        "core.status.choose",
        Box::new(|env, params| {
            let params = params
                .clone()
                .with(params::MODE, paperpaw_sdk::ui::items::LINES);
            Box::new(paperpaw_sdk::ui::items::ItemsScreen::open(env, &params))
        }),
    ))?;
    // The choice every plugin asks (`ask::choice`), unless another
    // screen fills the slot.
    r.add_screen(ScreenSpec::new(
        screens::CHOICE,
        "core.choice",
        Box::new(|env, params| Box::new(paperpaw_sdk::ui::choice::Choice::new(env, params))),
    ))?;
    r.bind_slot(slots::CHOICE, screens::CHOICE, Priority::FALLBACK)?;
    r.add_screen(ScreenSpec::new(
        screens::PLACEHOLDER,
        "core.screen.placeholder",
        Box::new(|_, _| Box::new(PlaceholderScreen)),
    ))?;
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
    r.add_menu_entry(MenuEntry::new(menus::TOOLS, actions::SCREENSHOT).order(95))?;
    r.add_icon(actions::SCREENSHOT, Cow::Borrowed("camera"))?;
    for (action, order, icon) in [
        (actions::HOME, 10, "home"),
        (actions::LIBRARY, 20, "books"),
        (actions::TOOLS, 30, "apps"),
        (settings::NIGHT_MODE.into(), 85, "moon"),
        (actions::SETTINGS, 90, "settings"),
    ] {
        r.add_menu_entry(MenuEntry::new(menus::MAIN, action.clone()).order(order))?;
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
        r.add_menu_entry(MenuEntry::new(menus::QUICK, item).order(order))?;
    }
    r.add_icon(
        actions::QUICK_PANEL,
        Cow::Borrowed("adjustments-horizontal"),
    )?;
    r.add_icon(actions::REFRESH_SCREEN, Cow::Borrowed("sparkles"))?;

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
        r.add_menu_section(
            paperpaw_sdk::MenuSection::new(id, title, menus::SETTINGS).order(order),
        )?;
    }
    for (id, title, order, icon) in [
        (groups::READING, "core.group.reading", 10, "book"),
        (groups::LIBRARY, "core.group.library", 20, "books"),
        (
            groups::STATUS_LINES,
            "core.group.status",
            15,
            "layout-bottombar",
        ),
        (groups::MARKS, "core.group.marks", 17, "highlight"),
        (groups::DISPLAY, "core.group.display", 60, "contrast"),
        (groups::CONTROLS, "core.group.controls", 70, "hand-finger"),
        (groups::SYSTEM, "core.group.system", 90, "device-tablet"),
        (groups::LOOK, "core.group.look", 210, "palette"),
        (groups::OTHER, "core.group.other", 100, "dots"),
        (groups::DEVELOPER, "core.group.developer", 95, "bug"),
    ] {
        r.add_setting_group(SettingGroupSpec::new(id.clone(), title).order(order))?;
        r.add_icon(id, Cow::Borrowed(icon))?;
    }
    Ok(())
}

/// Units, steps and sections of the kernel's settings, and the numbers
/// set by eye, with sliders.
static MM: SettingExtra = SettingExtra::new().unit("core.unit.mm");
static MM_SLIDER: SettingExtra = SettingExtra::new().unit("core.unit.mm").slider();
static PT_SLIDER: SettingExtra = SettingExtra::new().unit("core.unit.pt").slider();
static PAGES: SettingExtra = SettingExtra::new().unit("core.unit.pages");
static SECONDS: SettingExtra = SettingExtra::new().unit("core.unit.seconds");
static MS: SettingExtra = SettingExtra::new().unit("core.unit.ms");
static TENTH_MM: SettingExtra = SettingExtra::new().unit("core.unit.tenth-mm");
static PERCENT_10_SLIDER: SettingExtra = SettingExtra::new()
    .unit("core.unit.percent")
    .step(10)
    .slider();
static PERCENT_5: SettingExtra = SettingExtra::new().unit("core.unit.percent").step(5);
static MINUTES_5: SettingExtra = SettingExtra::new().unit("core.unit.minutes").step(5);
static COLORS: SettingExtra = SettingExtra::new().section("core.section.colors");
static FONT: SettingExtra = SettingExtra::new().section(setting_sections::FONT);
static FONT_PT: SettingExtra = SettingExtra::new()
    .section(setting_sections::FONT)
    .unit("core.unit.pt")
    .slider();

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
        SettingValue::Text(Cow::Borrowed(paperpaw_i18n::FALLBACK_LANGUAGE)),
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
        settings::MONO_TEXT_LEVELS,
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
    r.set_menu_need(settings::MONO_TEXT, paperpaw_sdk::MenuNeed::BlackAndWhite)?;
    r.set_menu_need(settings::SPEAK_FOCUS, paperpaw_sdk::MenuNeed::Speech)?;
    Ok(())
}

/// How the interface looks: its size and contrast for everyone; with
/// advanced settings its header, corners, highlight, lines, groups,
/// titles and room. The defaults are PaperPaw's look (soft corners,
/// pills, plain headers, faint lines); themes (data plugins) set others.
static LOOK: [SettingSpec; 13] = [
    SettingSpec::fixed_number(
        settings::UI_SIZE.static_str(),
        "core.setting.ui-size",
        7,
        24,
        9,
        groups::LOOK,
    )
    .with_extra(&PT_SLIDER)
    .described("core.setting.ui-size.help"),
    SettingSpec::fixed_choice(
        settings::UI_CONTRAST.static_str(),
        "core.setting.ui-contrast",
        &[settings::CONTRAST_NORMAL, settings::CONTRAST_STRONG],
        settings::CONTRAST_NORMAL,
        groups::LOOK,
    )
    .described("core.setting.ui-contrast.help"),
    SettingSpec::fixed_choice(
        settings::UI_HEADER.static_str(),
        "core.setting.ui-header",
        &[
            settings::HEADER_PLAIN,
            settings::HEADER_LIGHT,
            settings::HEADER_DARK,
        ],
        settings::HEADER_PLAIN,
        groups::LOOK,
    )
    .for_advanced()
    .described("core.setting.ui-header.help"),
    SettingSpec::fixed_choice(
        settings::UI_CORNERS.static_str(),
        "core.setting.ui-corners",
        settings::CORNERS,
        settings::CORNERS[3],
        groups::LOOK,
    )
    .for_advanced()
    .described("core.setting.ui-corners.help"),
    SettingSpec::fixed_choice(
        settings::UI_FOCUS.static_str(),
        "core.setting.ui-focus",
        &[
            settings::FOCUS_FILL,
            settings::FOCUS_RING,
            settings::FOCUS_SOFT,
        ],
        settings::FOCUS_FILL,
        groups::LOOK,
    )
    .for_advanced()
    .described("core.setting.ui-focus.help"),
    SettingSpec::fixed_choice(
        settings::UI_SEPARATORS.static_str(),
        "core.setting.ui-separators",
        &[
            settings::SEPARATORS_INSET,
            settings::SEPARATORS_LINE,
            settings::SEPARATORS_NONE,
        ],
        settings::SEPARATORS_INSET,
        groups::LOOK,
    )
    .for_advanced()
    .described("core.setting.ui-separators.help"),
    SettingSpec::fixed_choice(
        settings::UI_GROUPS.static_str(),
        "core.setting.ui-groups",
        &[settings::GROUPS_PLAIN, settings::GROUPS_CARDS],
        settings::GROUPS_PLAIN,
        groups::LOOK,
    )
    .for_advanced()
    .described("core.setting.ui-groups.help"),
    SettingSpec::fixed_number(
        settings::UI_TITLE.static_str(),
        "core.setting.ui-title",
        100,
        200,
        130,
        groups::LOOK,
    )
    .with_extra(&PERCENT_10_SLIDER)
    .for_advanced()
    .described("core.setting.ui-title.help"),
    SettingSpec::fixed_number(
        settings::UI_MARGIN.static_str(),
        "core.setting.ui-margin",
        0,
        15,
        4,
        groups::LOOK,
    )
    .with_extra(&MM_SLIDER)
    .for_advanced()
    .described("core.setting.ui-margin.help"),
    SettingSpec::fixed_number(
        settings::UI_ROW.static_str(),
        "core.setting.ui-row",
        120,
        300,
        160,
        groups::LOOK,
    )
    .with_extra(&PERCENT_10_SLIDER)
    .for_advanced()
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
    .for_advanced()
    .described("core.setting.ui-lines.help"),
    // A picture in place of PaperPaw's logo: for data plugins, which set
    // it in their settings.ini, and those who type a path.
    SettingSpec::fixed_text(
        paperpaw_sdk::logo::LOGO.static_str(),
        "core.setting.logo",
        "",
        groups::LOOK,
    )
    .for_advanced()
    .described("core.setting.logo.help"),
    // The interface's grays by what they are for: for themes, which set
    // it in their settings.ini.
    SettingSpec::fixed_text(
        settings::UI_PALETTE.static_str(),
        "core.setting.ui-palette",
        "",
        groups::LOOK,
    )
    .for_advanced()
    .described("core.setting.ui-palette.help"),
];

fn register_look_settings(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_settings(&LOOK)
}

/// Buttons, the panel's refresh, sleep, rotation, notices and long
/// presses.
static DEVICE: [SettingSpec; 9] = [
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
    .with_extra(&PAGES)
    .scoped()
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
        settings::ROTATIONS,
        settings::ROTATIONS[0],
        groups::DISPLAY,
    )
    .scoped()
    .described("core.setting.rotation.help"),
    SettingSpec::fixed_switch(
        settings::BUTTON_HINTS.static_str(),
        "core.setting.button-hints",
        true,
        groups::CONTROLS,
    )
    .described("core.setting.button-hints.help"),
    SettingSpec::fixed_switch(
        settings::SWAP_PAGE_BUTTONS.static_str(),
        "core.setting.swap-page-buttons",
        false,
        groups::CONTROLS,
    )
    .described("core.setting.swap-page-buttons.help"),
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
    r.set_menu_need(settings::SLEEP_AFTER, paperpaw_sdk::MenuNeed::Sleep)?;
    r.set_menu_need(settings::ROTATION, paperpaw_sdk::MenuNeed::Rotation)?;
    r.set_menu_need(settings::BUTTON_HINTS, paperpaw_sdk::MenuNeed::ButtonRow)?;
    for edge in [settings::EDGE_ENDS, settings::EDGE_SIDES] {
        r.set_menu_need(edge, paperpaw_sdk::MenuNeed::Touch)?;
    }
    Ok(())
}

/// The colours of text and paper, each with its own colour code.
static COLOR_SETTINGS: [SettingSpec; 4] = [
    SettingSpec::fixed_choice(
        settings::TEXT_COLOR.static_str(),
        "core.setting.text-color",
        settings::TEXT_COLORS,
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
        settings::PAPER_COLORS,
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
        r.set_menu_need(id, paperpaw_sdk::MenuNeed::Colors)?;
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
/// search hit, the sentence read aloud (see `paperpaw_sdk::decorations`).
mod marks {
    use paperpaw_sdk::SettingSpec;
    use paperpaw_sdk::decorations::{Decoration, LineKind, Tone};
    use paperpaw_sdk::role_settings;

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
    )
    .described("core.setting.clock-format.help"),
    SettingSpec::fixed_choice(
        settings::WEEK_START.static_str(),
        "core.setting.week-start",
        time::WEEK_STARTS,
        time::WEEK_STARTS[0],
        groups::SYSTEM,
    )
    .described("core.setting.week-start.help"),
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
    r.add_status_item(StatusItemSpec::new(
        status_items::CLOCK,
        "core.status.clock",
        Box::new(|env, _| time::clock_text(env).map(StatusValue::Text)),
    ))?;
    r.add_status_item(StatusItemSpec::new(
        status_items::DATE,
        "core.status.date",
        Box::new(|env, _| time::date_text(env).map(StatusValue::Text)),
    ))?;
    r.add_status_item(StatusItemSpec::new(
        status_items::DATE_SHORT,
        "core.status.date-short",
        Box::new(|env, _| time::short_date_text(env).map(StatusValue::Text)),
    ))?;
    r.add_status_item(StatusItemSpec::new(
        status_items::BATTERY,
        "core.status.battery",
        Box::new(|env, _| {
            env.status.battery.map(|b| StatusValue::Level {
                percent: b.percent,
                charging: b.charging,
            })
        }),
    ))?;
    r.add_status_item(StatusItemSpec::new(
        status_items::BATTERY_LOW,
        "core.status.battery-low",
        Box::new(|env, _| {
            let low = env.settings.read(&settings::BATTERY_LOW);
            env.status
                .battery
                .filter(|b| i64::from(b.percent) <= low)
                .map(|b| StatusValue::Level {
                    percent: b.percent,
                    charging: b.charging,
                })
        }),
    ))?;
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
    r.add_setup_step(
        paperpaw_sdk::setup::SetupStepSpec::new(
            Id::new_static("core.setup.language"),
            "",
            paperpaw_sdk::setup::SetupPage::Setting(settings::LANGUAGE.into()),
        )
        .order(10),
    )?;
    r.add_setup_step(
        paperpaw_sdk::setup::SetupStepSpec::new(
            Id::new_static("core.setup.buttons"),
            "",
            paperpaw_sdk::setup::SetupPage::Setting(settings::KEYMAP_PRESET.into()),
        )
        .order(20)
        .applies(Box::new(|env: &paperpaw_sdk::Env<'_>| {
            env.device.input.buttons && !env.device.input.touch
        })),
    )?;
    r.add_setup_step(
        paperpaw_sdk::setup::SetupStepSpec::new(
            Id::new_static("core.setup.time-zone"),
            "",
            paperpaw_sdk::setup::SetupPage::Setting(settings::TIME_ZONE.into()),
        )
        .help("core.setup.time-zone.help")
        .order(40)
        .applies(Box::new(|env: &paperpaw_sdk::Env<'_>| {
            env.status
                .clock
                .is_none_or(|c| c.utc_offset_minutes.is_none())
        })),
    )?;
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
/// named by the time ("PaperPaw 2026-10-15 17-30-05.png").
fn screenshot_path(env: &Env<'_>) -> alloc::string::String {
    let library = paperpaw_sdk::library::folder(env);
    let parent = paperpaw_sdk::files::parent(library);
    let dir = if parent.is_empty() {
        alloc::string::String::from("Screenshots")
    } else {
        alloc::format!("{parent}/Screenshots")
    };
    let _ = env.storage.create_dir(&dir);
    let name = match paperpaw_sdk::time::local_time(env) {
        Some(t) => alloc::format!(
            "PaperPaw {}-{:02}-{:02} {:02}-{:02}-{:02}",
            t.year,
            t.month,
            t.day,
            t.hour,
            t.minute,
            t.second
        ),
        None => alloc::format!("PaperPaw {}", env.now.0),
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
    widgets::SPACE_SIZES,
    widgets::SPACE_SIZES[1],
    Id::new_static(widgets::SPACE),
)
.described("core.setting.space-size.help")];

/// The widgets for room between the others: a gap of a set height, and
/// the room left (see `paperpaw_sdk::widgets`).
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
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// Every plugin writes numbers with the language's decimal mark
    /// (`paperpaw_sdk::numbers`), which the kernel's catalogs name.
    #[test]
    fn languages_name_their_decimal_mark() {
        use paperpaw_sdk::numbers::DECIMAL_MARK;
        for (source, mark) in [
            (include_str!("../i18n/en.ini"), "."),
            (include_str!("../i18n/de.ini"), ","),
        ] {
            let catalog = Catalog::parse(source).unwrap();
            assert_eq!(catalog.get(DECIMAL_MARK), Some(mark));
        }
    }
}
