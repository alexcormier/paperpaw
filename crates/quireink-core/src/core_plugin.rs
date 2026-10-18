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
    SettingGroupSpec, SettingKind, SettingSpec, SettingValue, SlotSpec, StatusItemSpec,
    StatusValue,
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
        // In the row of hints: arrows for moving, short words for the rest.
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
        // How a screen takes gestures (`gestures.policy` in key maps): ids
        // to bind, nothing to run.
        for (id, title) in [
            ("core.gestures.standard", "core.gestures.standard"),
            ("core.gestures.own", "core.gestures.own"),
            ("core.gestures.off", "core.gestures.off"),
        ] {
            r.add_action(ActionSpec::new(Id::new_static(id), title))?;
        }
        let handled: [(Id, &'static str, ActionHandler); 15] = [
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

        r.add_setting(
            SettingSpec::switch(&settings::DEVELOPER, "core.setting.developer", false)
                .group(groups::SYSTEM)
                .advanced()
                .description("core.setting.developer.help"),
        )?;
        r.add_setting(
            SettingSpec::language(
                &settings::LANGUAGE,
                "core.setting.language",
                quireink_i18n::FALLBACK_LANGUAGE,
            )
            .group(groups::SYSTEM)
            .description("core.setting.language.help"),
        )?;
        r.add_setting(
            SettingSpec::switch(&settings::NIGHT_MODE, "core.setting.night-mode", false)
                .group(groups::DISPLAY)
                .description("core.setting.night-mode.help"),
        )?;
        r.add_setting(
            SettingSpec::switch(&settings::SPEAK_FOCUS, "core.setting.speak-focus", false)
                .group(groups::SYSTEM)
                .description("core.setting.speak-focus.help"),
        )?;
        r.set_menu_need(settings::SPEAK_FOCUS, quireink_sdk::MenuNeed::Speech)?;
        for (id, title, default, max) in [
            (settings::EDGE_ENDS, "core.setting.edge-ends", 10, 25),
            (settings::EDGE_SIDES, "core.setting.edge-sides", 5, 25),
        ] {
            r.add_setting(
                SettingSpec::new(
                    id.clone(),
                    title,
                    SettingKind::Int { min: 0, max },
                    SettingValue::Int(default),
                )
                .group(groups::CONTROLS)
                .unit("core.unit.mm")
                .advanced()
                .description("core.setting.edge.help"),
            )?;
        }
        r.add_setting(
            SettingSpec::number(&settings::UI_SIZE, "core.setting.ui-size", 7, 24, 9)
                .group(groups::LOOK)
                .unit("core.unit.pt")
                .description("core.setting.ui-size.help"),
        )?;
        r.add_setting(
            SettingSpec::choice(
                &settings::UI_HEADER,
                "core.setting.ui-header",
                alloc::vec![
                    Cow::Borrowed(settings::HEADER_DARK),
                    Cow::Borrowed(settings::HEADER_LIGHT),
                ],
                settings::HEADER_LIGHT,
            )
            .group(groups::LOOK)
            .description("core.setting.ui-header.help"),
        )?;
        r.add_setting(
            SettingSpec::choice(
                &settings::UI_CORNERS,
                "core.setting.ui-corners",
                settings::CORNERS
                    .iter()
                    .map(|c| Cow::Borrowed(*c))
                    .collect(),
                settings::CORNERS[0],
            )
            .group(groups::LOOK)
            .description("core.setting.ui-corners.help"),
        )?;
        r.add_setting(
            SettingSpec::choice(
                &settings::UI_CONTRAST,
                "core.setting.ui-contrast",
                alloc::vec![
                    Cow::Borrowed(settings::CONTRAST_NORMAL),
                    Cow::Borrowed(settings::CONTRAST_STRONG),
                ],
                settings::CONTRAST_NORMAL,
            )
            .group(groups::LOOK)
            .description("core.setting.ui-contrast.help"),
        )?;
        for (id, title, (min, max, default), unit, step) in [
            (
                settings::UI_MARGIN,
                "core.setting.ui-margin",
                (0, 15, 3),
                "core.unit.mm",
                1,
            ),
            (
                settings::UI_ROW,
                "core.setting.ui-row",
                (120, 300, 180),
                "core.unit.percent",
                10,
            ),
            (
                settings::UI_LINES,
                "core.setting.ui-lines",
                (1, 10, 3),
                "core.unit.tenth-mm",
                1,
            ),
        ] {
            r.add_setting(
                SettingSpec::new(
                    id,
                    title,
                    SettingKind::Int { min, max },
                    SettingValue::Int(default),
                )
                .group(groups::LOOK)
                .unit(unit)
                .step(step)
                .description(match title {
                    "core.setting.ui-margin" => "core.setting.ui-margin.help",
                    "core.setting.ui-row" => "core.setting.ui-row.help",
                    _ => "core.setting.ui-lines.help",
                }),
            )?;
        }
        r.add_setting(
            SettingSpec::choice(
                &settings::KEYMAP_PRESET,
                "core.setting.keymap-preset",
                alloc::vec![Cow::Borrowed(keymap_presets::STANDARD_NAME)],
                keymap_presets::STANDARD_NAME,
            )
            .group(groups::CONTROLS)
            .description("core.setting.keymap-preset.help"),
        )?;
        r.add_setting(
            SettingSpec::number(
                &settings::FULL_REFRESH_INTERVAL,
                "core.setting.full-refresh-interval",
                0,
                1000,
                6,
            )
            .group(groups::DISPLAY)
            .advanced()
            .unit("core.unit.pages")
            .description("core.setting.full-refresh-interval.help"),
        )?;
        r.add_setting(
            SettingSpec::switch(&settings::FLASH_SCREENS, "core.setting.flash-screens", true)
                .group(groups::DISPLAY)
                .advanced()
                .description("core.setting.flash-screens.help"),
        )?;
        r.add_setting(
            SettingSpec::number(
                &settings::SLEEP_AFTER,
                "core.setting.sleep-after",
                0,
                120,
                10,
            )
            .group(groups::SYSTEM)
            .unit("core.unit.minutes")
            .step(5)
            .description("core.setting.sleep-after.help"),
        )?;
        r.set_menu_need(settings::SLEEP_AFTER, quireink_sdk::MenuNeed::Sleep)?;
        r.add_setting(
            SettingSpec::choice(
                &settings::ROTATION,
                "core.setting.rotation",
                settings::ROTATIONS
                    .iter()
                    .map(|r| Cow::Borrowed(*r))
                    .collect(),
                settings::ROTATIONS[0],
            )
            .group(groups::DISPLAY)
            .description("core.setting.rotation.help"),
        )?;
        r.set_menu_need(settings::ROTATION, quireink_sdk::MenuNeed::Rotation)?;
        r.add_setting(
            SettingSpec::switch(&settings::BUTTON_HINTS, "core.setting.button-hints", true)
                .group(groups::CONTROLS)
                .description("core.setting.button-hints.help"),
        )?;
        r.set_menu_need(settings::BUTTON_HINTS, quireink_sdk::MenuNeed::ButtonRow)?;
        register_spaces(r)?;
        // Each colour, then its own colour code.
        for (id, title, values, own, own_title, own_default) in [
            (
                settings::TEXT_COLOR,
                "core.setting.text-color",
                &settings::TEXT_COLORS,
                settings::TEXT_COLOR_OWN,
                "core.setting.text-color-own",
                "#5b4636",
            ),
            (
                settings::PAPER_COLOR,
                "core.setting.paper-color",
                &settings::PAPER_COLORS,
                settings::PAPER_COLOR_OWN,
                "core.setting.paper-color-own",
                "#f4ecd8",
            ),
        ] {
            r.add_setting(
                SettingSpec::new(
                    id.clone(),
                    title,
                    SettingKind::Choice(values.iter().map(|v| Cow::Borrowed(*v)).collect()),
                    SettingValue::Text(Cow::Borrowed(values[0])),
                )
                .group(groups::LOOK)
                .section("core.section.colors")
                .description("core.setting.colors.help"),
            )?;
            r.set_menu_need(id, quireink_sdk::MenuNeed::Colors)?;
            r.add_setting(
                SettingSpec::new(
                    own.clone(),
                    own_title,
                    SettingKind::Text,
                    SettingValue::Text(Cow::Borrowed(own_default)),
                )
                .group(groups::LOOK)
                .section("core.section.colors")
                .description("core.setting.color-own.help"),
            )?;
            r.set_menu_need(own, quireink_sdk::MenuNeed::Colors)?;
        }
        r.add_setting(
            SettingSpec::number(
                &settings::NOTICE_SECONDS,
                "core.setting.notice-seconds",
                1,
                60,
                4,
            )
            .group(groups::DISPLAY)
            .advanced()
            .unit("core.unit.seconds")
            .description("core.setting.notice-seconds.help"),
        )?;
        r.add_setting(
            SettingSpec::number(
                &settings::LONG_PRESS_MS,
                "core.setting.long-press-ms",
                200,
                3000,
                500,
            )
            .group(groups::CONTROLS)
            .advanced()
            .unit("core.unit.ms")
            .description("core.setting.long-press-ms.help"),
        )?;

        r.add_setting(
            SettingSpec::choice(
                &settings::READING_FONT,
                "core.setting.reading-font",
                alloc::vec![Cow::Borrowed(settings::DEFAULT_FONT)],
                settings::DEFAULT_FONT,
            )
            .group(groups::READING)
            .section(setting_sections::FONT)
            .scoped()
            .description("core.setting.reading-font.help"),
        )?;
        r.add_setting(
            SettingSpec::switch(&settings::SCOPE_ONLY, "core.setting.scope-only", false)
                .group(groups::READING)
                .scoped()
                .description("core.setting.scope-only.help"),
        )?;
        // The time of day and the battery, for status lines.
        r.add_setting(
            SettingSpec::choice(
                &settings::CLOCK_FORMAT,
                "core.setting.clock-format",
                alloc::vec![Cow::Borrowed(CLOCK_24H), Cow::Borrowed(CLOCK_12H)],
                CLOCK_24H,
            )
            .group(groups::SYSTEM),
        )?;
        let mut zones = alloc::vec![Cow::Borrowed(AUTO)];
        zones.extend(TIME_ZONES.iter().map(|(id, _)| Cow::Borrowed(*id)));
        r.add_setting(
            SettingSpec::choice(&settings::TIME_ZONE, "core.setting.time-zone", zones, AUTO)
                .group(groups::SYSTEM)
                .description("core.setting.time-zone.help"),
        )?;
        r.add_setting(
            SettingSpec::text(&settings::TIME_ZONE_RULE, "core.setting.time-zone-rule", "")
                .group(groups::SYSTEM)
                .advanced()
                .description("core.setting.time-zone-rule.help"),
        )?;
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
        r.add_setting(
            SettingSpec::number(
                &settings::BATTERY_LOW,
                "core.setting.battery-low",
                5,
                50,
                20,
            )
            .group(groups::SYSTEM)
            .unit("core.unit.percent")
            .step(5)
            .advanced()
            .description("core.setting.battery-low.help"),
        )?;

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

/// The widgets for room between the others: a gap of a set height, and
/// the room left (see `quireink_sdk::widgets`).
fn register_spaces(r: &mut Registrar) -> Result<(), RegisterError> {
    let space = Id::new_static(widgets::SPACE);
    r.add_setting_group(widgets::settings_group(&space, "core.widget.space"))?;
    r.add_setting(
        SettingSpec::choice(
            &widgets::SPACE_SIZE,
            "core.setting.space-size",
            widgets::SPACE_SIZES
                .iter()
                .map(|s| Cow::Borrowed(*s))
                .collect(),
            widgets::SPACE_SIZES[1],
        )
        .group(space.clone())
        .description("core.setting.space-size.help"),
    )?;
    let space_height = |env: &Env<'_>, _| {
        let size = env.settings.read(&widgets::SPACE_SIZE);
        let mm = match widgets::SPACE_SIZES.iter().position(|s| *s == size) {
            Some(0) => 4,
            Some(2) => 20,
            _ => 10,
        };
        env.metrics.mm(mm).min(u32::from(u16::MAX)) as u16
    };
    r.add_widget(WidgetSpec::drawn(
        space,
        "core.widget.space",
        Drawn::new(space_height, |_, _, _| {}),
    ))?;
    // Screens that place widgets give it the room left.
    r.add_widget(WidgetSpec::drawn(
        Id::new_static(widgets::FILL),
        "core.widget.fill",
        Drawn::new(|_, _| 0, |_, _, _| {}),
    ))?;
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
