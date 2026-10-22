//! The sleep screen (slot `core.sleep`): what the screen shows while the
//! device sleeps, e-ink keeping it without power.
//!
//! Setting `sleep.show`:
//!
//! - *Cover* (the default): the cover of the book read last;
//! - *Picture*: the pictures (JPEG, PNG) of a folder (`sleep.folder`,
//!   `sleep` at first, as on CrossPoint), one after the other;
//! - *Message*: PaperPaw's logo and "Sleeping" (the logo as set,
//!   see [`paperpaw_sdk::logo`]);
//! - *Widgets*: widgets as on the home screen (setting `sleep.widgets`, a
//!   place of widgets above each other, edited on the slot
//!   `core.edit-widgets`), e.g. the book being read and today's reading,
//!   in the middle of the screen;
//! - *Page*: the screen stays as it is;
//! - *Page with a picture*: the page stays, a picture or a widget over
//!   it, its clear parts showing the page (see [`overlay`]);
//! - a mode of another plugin: it offers a screen of its own among the
//!   choices (`Registrar::add_choice(sleep::SHOW, "my.sleep-screen")`, the
//!   value the screen's id and the translation key of its name), which
//!   the sleep screen shows in its place, sleeping as it does.
//!
//! Without a cover or a picture the message shows. Where the device is
//! off while it sleeps (the X4), this is also what shows as it starts.
//! Asleep, the screen takes every button but *Sleep* and *Home*. It
//! closes when the device wakes, wherever it is: what opened above it as
//! the device woke (a lock, a review of flash cards) stays. Only where PaperPaw
//! shows the sleep screen itself (the X4, not Android): elsewhere its
//! settings are hidden.

#![no_std]

extern crate alloc;

pub mod overlay;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, groups, slots};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::library;
use paperpaw_sdk::logo::Logo;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{PlaceKind, Stack, WidgetPlace};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Handled, Id, Manifest, MenuNeed, Params, Plugin, Priority,
    Rect, RefreshMode, RegisterError, Registrar, Screen, ScreenSpec, SettingSpec, ShowsThrough,
    Version,
};

pub const PLUGIN: Id = Id::new_static("sleep");
/// What the sleep screen shows: one of [`COVER`], [`PICTURE`],
/// [`OVERLAY`], [`WIDGETS`], [`MESSAGE`], [`PAGE`], or a screen another
/// plugin offers ([`paperpaw_sdk::Registrar::add_choice`]).
pub const SHOW: Setting<Text> = Setting::new("sleep.show");
pub const COVER: &str = "sleep.show.cover";
pub const PICTURE: &str = "sleep.show.picture";
pub const MESSAGE: &str = "sleep.show.message";
pub const PAGE: &str = "sleep.show.page";
pub const WIDGETS: &str = "sleep.show.widgets";
/// The page, a picture over it (see [`overlay`]).
pub const OVERLAY: &str = "sleep.show.overlay";
/// The widgets shown with [`WIDGETS`], ids separated by commas.
pub const WIDGET_LIST: Setting<Text> = Setting::new("sleep.widgets");
/// The folder of the pictures.
pub const FOLDER: Setting<Text> = Setting::new("sleep.folder");
const SCREEN: Id = Id::new_static("sleep.screen");
const SECTION: &str = "sleep.section";

pub(crate) fn data(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// The sleep screen plugin.
#[derive(Debug, Default)]
pub struct Sleep;

/// The sleep screen's part of the display settings, its folder and its
/// widgets each chosen on a screen of their own.
static IN_SLEEP: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().section(SECTION);
static FOLDER_EXTRA: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .section(SECTION)
    .editor_slot(paperpaw_sdk::builtin::slots::CHOOSE_FOLDER);
static WIDGETS_EXTRA: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .section(SECTION)
    .editor_slot(slots::EDIT_WIDGETS);
/// Where its widgets show, for editors and widgets that ask.
const PLACE: WidgetPlace = WidgetPlace::new(
    Id::new_static("sleep.widgets"),
    "sleep.title",
    PLACE_SETTINGS,
    PlaceKind::Stack,
)
.shown_if(Id::new_static(SHOW.static_str()), WIDGETS);
const PLACE_SETTINGS: &[Id] = &[Id::new_static(WIDGET_LIST.static_str())];
/// The widget shown over the page, where one is (see [`overlay`]).
const OVERLAY_PLACE: WidgetPlace = WidgetPlace::new(
    Id::new_static("sleep.overlay"),
    "sleep.title",
    OVERLAY_SETTINGS,
    PlaceKind::Stack,
)
.shown_if(Id::new_static(SHOW.static_str()), OVERLAY);
const OVERLAY_SETTINGS: &[Id] = &[Id::new_static(overlay::WIDGET.static_str())];
static PICKED_EXTRA: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .section(SECTION)
    .editor(overlay::PICK);
static PERCENT: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .section(SECTION)
    .unit("core.unit.percent")
    .step(5)
    .slider();

/// What the sleep screen shows, the folder of its pictures, its widgets
/// and its picture over the page.
static SETTINGS: [SettingSpec; 9] = [
    SettingSpec::fixed_choice(
        SHOW.static_str(),
        "sleep.setting.show",
        &[COVER, PICTURE, OVERLAY, WIDGETS, MESSAGE, PAGE],
        COVER,
        groups::DISPLAY,
    )
    .with_extra(&IN_SLEEP)
    .described("sleep.setting.show.help"),
    SettingSpec::fixed_text(
        FOLDER.static_str(),
        "sleep.setting.folder",
        "sleep",
        groups::DISPLAY,
    )
    .with_extra(&FOLDER_EXTRA)
    .described("sleep.setting.folder.help"),
    SettingSpec::fixed(
        WIDGET_LIST.static_str(),
        "sleep.setting.widgets",
        paperpaw_sdk::SettingKind::List,
        paperpaw_sdk::SettingValue::Text(Cow::Borrowed(
            "home.date, home.continue, stats.widget.today, home.recent",
        )),
        groups::DISPLAY,
    )
    .with_extra(&WIDGETS_EXTRA)
    .described("sleep.setting.widgets.help"),
    SettingSpec::fixed_text(
        overlay::PICTURE.static_str(),
        "sleep.setting.overlay-picture",
        "",
        groups::DISPLAY,
    )
    .with_extra(&PICKED_EXTRA)
    .described("sleep.setting.overlay-picture.help"),
    SettingSpec::fixed_text(
        overlay::WIDGET.static_str(),
        "sleep.setting.overlay-widget",
        "",
        groups::DISPLAY,
    )
    .with_extra(&PICKED_EXTRA)
    .described("sleep.setting.overlay-widget.help"),
    SettingSpec::fixed_choice(
        overlay::POSITION.static_str(),
        "sleep.setting.overlay-position",
        &overlay::POSITIONS,
        overlay::DEFAULT_POSITION,
        groups::DISPLAY,
    )
    .with_extra(&IN_SLEEP)
    .described("sleep.setting.overlay-position.help"),
    SettingSpec::fixed_number(
        overlay::SIZE.static_str(),
        "sleep.setting.overlay-size",
        10,
        100,
        overlay::DEFAULT_SIZE,
        groups::DISPLAY,
    )
    .with_extra(&PERCENT)
    .described("sleep.setting.overlay-size.help"),
    SettingSpec::fixed_number(
        overlay::SEE_THROUGH.static_str(),
        "sleep.setting.overlay-see-through",
        0,
        90,
        0,
        groups::DISPLAY,
    )
    .with_extra(&PERCENT)
    .described("sleep.setting.overlay-see-through.help"),
    SettingSpec::fixed_choice(
        overlay::USED.static_str(),
        "sleep.setting.overlay-when",
        &overlay::WHEN,
        overlay::WHEN[0],
        groups::DISPLAY,
    )
    .with_extra(&IN_SLEEP)
    .described("sleep.setting.overlay-when.help"),
];

impl Plugin for Sleep {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Sleep screen", Version::new(0, 1, 0))
            .description(
                "Shows the book's cover, your pictures or a message while the device sleeps.",
            )
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        let items: [Id; 9] = [
            SHOW.into(),
            FOLDER.into(),
            WIDGET_LIST.into(),
            overlay::PICTURE.into(),
            overlay::WIDGET.into(),
            overlay::POSITION.into(),
            overlay::SIZE.into(),
            overlay::SEE_THROUGH.into(),
            overlay::USED.into(),
        ];
        for item in items {
            r.set_menu_need(item, MenuNeed::Sleep)?;
        }
        r.add_screen(ScreenSpec::new(
            overlay::PICK,
            "sleep.setting.overlay-picture",
            Box::new(|env, params| Box::new(overlay::Picker::new(env, params))),
        ))?;
        // Asleep, the buttons do nothing but wake the device.
        r.set_screen_hints(SCREEN, paperpaw_sdk::Hints::Hidden)?;
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "sleep.title",
            Box::new(|env, _| Box::new(SleepScreen::new(env))),
        ))?;
        r.bind_slot(slots::SLEEP, SCREEN, Priority::DEFAULT)?;
        r.add_widget_place(PLACE)?;
        r.add_widget_place(OVERLAY_PLACE)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The cover of the book read last, as the library knows it: the book is
/// opened for it only once (memory is short while a book is open).
fn cover(env: &Env<'_>) -> Option<ImageRef> {
    let path = library::opened_books(env.storage).into_iter().next()?;
    library::indexed(env, &path)
        .cover
        .filter(|image| env.storage.exists(&image.path))
}

/// The next picture of the folder, decoded to fit the screen.
fn picture(env: &Env<'_>) -> Option<ImageRef> {
    let path = next_from_folder(env)?;
    let display = env.device.display;
    let fit = paperpaw_sdk::image::Fit::screen(display.width.into(), display.height.into());
    // In colour on colour screens (ADR 0015).
    paperpaw_sdk::image::picture_from_file(
        env.storage,
        &path,
        &data("picture.gray"),
        fit,
        env.device.display.format.is_color(),
    )
}

/// The path of the next picture of the pictures folder, one after the
/// other from where the last sleep left off.
pub(crate) fn next_from_folder(env: &Env<'_>) -> Option<String> {
    let folder = env
        .settings
        .text(FOLDER.as_str())
        .trim_matches('/')
        .to_string();
    let mut pictures = Vec::new();
    let _ = env.storage.list(&folder, &mut |entry| {
        let name = entry.name.to_lowercase();
        if !entry.is_dir
            && !entry.name.starts_with('.')
            && [".jpg", ".jpeg", ".png", ".gif"]
                .iter()
                .any(|e| name.ends_with(e))
        {
            pictures.push(entry.name.to_string());
        }
    });
    if pictures.is_empty() {
        return None;
    }
    pictures.sort_unstable();
    // One after the other, from where the last sleep left off.
    let next = next_picture(env.storage);
    let name = &pictures[next % pictures.len()];
    let _ = env.storage.write(
        &data("next.txt"),
        format!("{}", next.wrapping_add(1)).as_bytes(),
    );
    Some(format!("{folder}/{name}"))
}

/// What the screen shows while the device sleeps.
struct SleepScreen {
    show: String,
    image: Option<ImageRef>,
    /// What goes over the page, where, how opaque.
    over_page: Option<(overlay::Over, Rect, u8)>,
    /// Another plugin's screen shown in its place (its mode).
    shown: Option<Box<dyn Screen>>,
    /// The widgets shown, where they go.
    widgets: Stack,
    /// The logo above the message, when the message shows.
    logo: Option<Logo>,
    /// Whether the device woke since the screen opened.
    woke: bool,
}

impl SleepScreen {
    fn new(env: &Env<'_>) -> Self {
        let mut show = env.settings.read(&SHOW).to_string();
        // What goes over the page where it is used now, and there is
        // something; else the cover. Why not is kept for the picker.
        let mut over_page = None;
        if show == OVERLAY {
            if overlay::used(env) {
                match overlay::over(env) {
                    Ok(mut over) => {
                        let area = overlay::place(env, &mut over);
                        over_page = Some((over, area, overlay::opacity(env)));
                        overlay::remember_failure(env, None);
                    }
                    Err(why) => overlay::remember_failure(env, Some(&why)),
                }
            }
            if over_page.is_none() {
                show = COVER.to_string();
            }
        }
        // A mode of another plugin: its screen.
        let shown = (!BUILT_IN.contains(&show.as_str()))
            .then(|| Id::parse(&show).ok())
            .flatten()
            .and_then(|id| env.make_screen(&id, &Params::new()));
        let image = match show.as_str() {
            _ if shown.is_some() => None,
            OVERLAY => None,
            PICTURE => picture(env),
            MESSAGE | PAGE | WIDGETS => None,
            _ => cover(env),
        };
        let display = env.device.display;
        let screen = Rect::new(0, 0, display.width, display.height);
        let list = if show == WIDGETS {
            env.settings.read(&WIDGET_LIST)
        } else {
            ""
        };
        let widgets = Stack::new(env, list, screen);
        let message = image.is_none()
            && widgets.is_empty()
            && show != PAGE
            && over_page.is_none()
            && shown.is_none();
        let logo = message.then(|| {
            let side = logo_side(env);
            Logo::read(env, side, side)
        });
        Self {
            show,
            image,
            over_page,
            shown,
            widgets,
            logo,
            woke: false,
        }
    }
}

impl Screen for SleepScreen {
    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        if let Some(shown) = &self.shown {
            return shown.region(env);
        }
        // The page stays: nothing is drawn over it, or the picture over
        // all of it, the page drawn below (`shows_through`).
        let display = env.device.display;
        match self.show.as_str() {
            PAGE => Some(Rect::new(0, 0, 0, 0)),
            OVERLAY => Some(Rect::new(0, 0, display.width, display.height)),
            _ => None,
        }
    }

    fn shows_through(&self) -> ShowsThrough {
        if let Some(shown) = &self.shown {
            return shown.shows_through();
        }
        // The page, not a menu left open over it.
        if self.over_page.is_some() {
            ShowsThrough::FullScreen
        } else {
            ShowsThrough::Nothing
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        if let Some(shown) = &mut self.shown {
            return shown.draw(canvas, env);
        }
        if self.show == PAGE {
            return;
        }
        if let Some((over, area, opacity)) = &self.over_page {
            overlay::draw(canvas, env, over, *area, *opacity);
            return;
        }
        let display = env.device.display;
        let screen = Rect::new(0, 0, display.width, display.height);
        if !self.widgets.is_empty() {
            self.widgets.draw(canvas, env);
            return;
        }
        match &self.image {
            Some(image) => {
                // As large as fits, in the middle, on black.
                canvas.fill(screen, Color::BLACK);
                let (w, h) = ui::fit_image(image, display.width, display.height);
                let area = Rect::new((display.width - w) / 2, (display.height - h) / 2, w, h);
                ui::draw_image(canvas, env.storage, image, area);
            }
            None => {
                let style = UiStyle::new(env);
                let text = env.i18n.tr("sleep.sleeping");
                let Some(logo) = &self.logo else {
                    ui::draw_message(canvas, env, &style, screen, text);
                    return;
                };
                // The logo a little above the middle, the message below it.
                let side = logo_side(env);
                let (_, h) = logo.fit(side, side);
                let line = paperpaw_sdk::widgets::line_height(env, style.text);
                let gap = style.gap as u16 * 2;
                let top = display.height.saturating_sub(h + gap + line) / 2;
                let area = Rect::new(0, top, display.width, h);
                logo.draw(canvas, env.storage, area, Color::BLACK);
                let below = Rect::new(0, top + h + gap, display.width, line);
                ui::draw_message(canvas, env, &style, below, text);
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Shown again after waking, once what opened above it closed: it
        // goes too, so that it never keeps an awake device to itself.
        if self.woke {
            cx.command(Command::Back);
            return;
        }
        if self.show != PAGE {
            cx.request_refresh(RefreshMode::Full);
        }
        if let Some(shown) = &mut self.shown {
            shown.on_show(cx);
        }
    }

    fn on_hide(&mut self) {
        if let Some(shown) = &mut self.shown {
            shown.on_hide();
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if let Some(shown) = &mut self.shown {
            shown.on_timer(cx);
        }
    }

    /// A cover or a picture (also over the page): in grays where the
    /// panel can.
    fn grays(&self) -> bool {
        if let Some(shown) = &self.shown {
            return shown.grays();
        }
        let picture_over = matches!(self.over_page, Some((overlay::Over::Picture(_), ..)));
        picture_over || (self.show != PAGE && self.image.is_some() && self.widgets.is_empty())
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        // Awake and still here (see `on_show`): gone at the next press.
        if self.woke {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        // Nothing to do while asleep but sleep again or go home.
        if *action == actions::SLEEP || *action == actions::HOME {
            Handled::No
        } else {
            Handled::Yes
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(shown) = &mut self.shown {
            shown.on_event(event, params, cx);
        }
        if *event == events::RESUME {
            // Awake: gone, wherever it is. What opened above it as the
            // device woke (a lock, a review) stays where it is.
            self.woke = true;
            cx.close();
            if self.show != PAGE {
                cx.request_refresh(RefreshMode::Full);
            }
        }
    }
}

/// The modes of this plugin; others name another plugin's screen.
const BUILT_IN: [&str; 6] = [COVER, PICTURE, OVERLAY, WIDGETS, MESSAGE, PAGE];

/// The logo's size on the sleep screen: a third of the screen across.
fn logo_side(env: &Env<'_>) -> u16 {
    let display = env.device.display;
    display.width.min(display.height) / 3
}

/// Which picture comes next, counted on from the first (`next.txt`).
fn next_picture(storage: &dyn paperpaw_sdk::Storage) -> usize {
    paperpaw_sdk::read_file(storage, &data("next.txt"), 32)
        .ok()
        .and_then(|n| String::from_utf8(n).ok()?.trim().parse::<usize>().ok())
        .unwrap_or(0)
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

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn the_next_picture_of_version_1_is_read() {
        let storage = paperpaw_sdk::MemoryStorage::new().with(
            &super::data("next.txt"),
            include_str!("../testdata/next.txt.v1"),
        );
        assert_eq!(super::next_picture(&storage), 7);
    }
}
