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
//! - *Widgets*: widgets as on the home screen (setting `sleep.widgets`,
//!   edited with the home screen's editor), e.g. the book being read and
//!   today's reading, in the middle of the screen;
//! - *Page*: the screen stays as it is.
//!
//! Without a cover or a picture the message shows. Where the device is
//! off while it sleeps (the X4), this is also what shows as it starts.
//! Asleep, the screen takes every button but *Sleep* and *Home*. It
//! closes when the device wakes or, where a screen opened above it as the
//! device woke (a lock), as soon as that one closes. Only where PaperPaw
//! shows the sleep screen itself (the X4, not Android): elsewhere its
//! settings are hidden.

#![no_std]

extern crate alloc;

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
use paperpaw_sdk::widgets::Widget;
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Handled, Id, Manifest, MenuNeed, Params, Plugin, Priority,
    Rect, RefreshMode, RegisterError, Registrar, SDK_VERSION, Screen, ScreenSpec, SettingSpec,
    Version,
};

pub const PLUGIN: Id = Id::new_static("sleep");
/// What the sleep screen shows: one of [`COVER`], [`PICTURE`],
/// [`MESSAGE`], [`PAGE`].
pub const SHOW: Setting<Text> = Setting::new("sleep.show");
pub const COVER: &str = "sleep.show.cover";
pub const PICTURE: &str = "sleep.show.picture";
pub const MESSAGE: &str = "sleep.show.message";
pub const PAGE: &str = "sleep.show.page";
pub const WIDGETS: &str = "sleep.show.widgets";
/// The widgets shown with [`WIDGETS`], ids separated by commas.
pub const WIDGET_LIST: Setting<Text> = Setting::new("sleep.widgets");
/// The home screen's editor of lists of widgets, if installed.
const EDIT_WIDGETS: Id = Id::new_static("home.edit");
/// The folder of the pictures.
pub const FOLDER: Setting<Text> = Setting::new("sleep.folder");
const SCREEN: Id = Id::new_static("sleep.screen");
const SECTION: &str = "sleep.section";
/// The folder chooser of the `files` plugin, if installed.
const CHOOSE_FOLDER: Id = Id::new_static("files.choose-folder");

fn data(name: &str) -> String {
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
    .editor(CHOOSE_FOLDER);
static WIDGETS_EXTRA: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .section(SECTION)
    .editor(EDIT_WIDGETS);

/// What the sleep screen shows, the folder of its pictures and its
/// widgets.
static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::fixed_choice(
        SHOW.static_str(),
        "sleep.setting.show",
        &[COVER, PICTURE, WIDGETS, MESSAGE, PAGE],
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
];

impl Plugin for Sleep {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Sleep screen"),
            description: Cow::Borrowed(
                "Shows the book's cover, your pictures or a message while the device sleeps.",
            ),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NOTHING,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        for item in [SHOW, FOLDER, WIDGET_LIST] {
            r.set_menu_need(item, MenuNeed::Sleep)?;
        }
        // Asleep, the buttons do nothing but wake the device.
        r.set_screen_hints(SCREEN, paperpaw_sdk::Hints::Hidden)?;
        r.add_screen(ScreenSpec {
            id: SCREEN,
            title: Cow::Borrowed("sleep.title"),
            factory: Box::new(|env, _| Box::new(SleepScreen::new(env))),
        })?;
        r.bind_slot(slots::SLEEP, SCREEN, Priority::DEFAULT)?;
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
    let _ = env
        .storage
        .write(&data("next.txt"), format!("{}", next + 1).as_bytes());
    let display = env.device.display;
    let fit = paperpaw_sdk::image::Fit::screen(display.width.into(), display.height.into());
    // In colour on colour screens (ADR 0015).
    paperpaw_sdk::image::picture_from_file(
        env.storage,
        &format!("{folder}/{name}"),
        &data("picture.gray"),
        fit,
        env.device.display.format.is_color(),
    )
}

/// The widgets of [`WIDGET_LIST`] between the margins, together in the
/// middle of the screen, or where free spaces push them: each with where
/// it goes, what it shows read (none if none shows).
fn place_widgets(env: &Env<'_>) -> Vec<(Box<dyn Widget>, Rect)> {
    let style = UiStyle::new(env);
    let display = env.device.display;
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let width = display.width.saturating_sub(2 * margin);
    // Free spaces (`None`) share the room the others leave.
    let mut made: Vec<Option<(Box<dyn Widget>, u16)>> = Vec::new();
    let mut total = 0u16;
    let mut shown = 0;
    for id in paperpaw_sdk::status::item_ids(env.settings.read(&WIDGET_LIST)) {
        if id == paperpaw_sdk::widgets::FILL {
            made.push(None);
            continue;
        }
        let Some(widget) = env.widgets.make(id, env) else {
            continue;
        };
        let height = widget.height(env, width);
        if height == 0 {
            continue;
        }
        let added = height + if shown == 0 { 0 } else { gap * 2 };
        if total + added > display.height {
            break;
        }
        total += added;
        shown += 1;
        made.push(Some((widget, height)));
    }
    if shown == 0 {
        return Vec::new();
    }
    let fills = made.iter().filter(|m| m.is_none()).count();
    let fill =
        paperpaw_sdk::widgets::fill_height(u32::from(display.height), u32::from(total), fills);
    let mut y = if fills == 0 {
        (display.height - total) / 2
    } else {
        0
    };
    let mut placed = Vec::new();
    for made in made {
        let Some((mut widget, height)) = made else {
            y += fill;
            continue;
        };
        if !placed.is_empty() {
            y += gap * 2;
        }
        let area = Rect::new(margin, y, width, height);
        widget.place(env, area);
        placed.push((widget, area));
        y += height;
    }
    placed
}

/// What the screen shows while the device sleeps.
struct SleepScreen {
    show: String,
    image: Option<ImageRef>,
    /// The widgets shown, where they go.
    widgets: Vec<(Box<dyn Widget>, Rect)>,
    /// The logo above the message, when the message shows.
    logo: Option<Logo>,
    /// Whether the device woke since the screen opened.
    woke: bool,
}

impl SleepScreen {
    fn new(env: &Env<'_>) -> Self {
        let show = env.settings.read(&SHOW).to_string();
        let image = match show.as_str() {
            PICTURE => picture(env),
            MESSAGE | PAGE | WIDGETS => None,
            _ => cover(env),
        };
        let widgets = if show == WIDGETS {
            place_widgets(env)
        } else {
            Vec::new()
        };
        let message = image.is_none() && widgets.is_empty() && show != PAGE;
        let logo = message.then(|| {
            let side = logo_side(env);
            Logo::read(env, side, side)
        });
        Self {
            show,
            image,
            widgets,
            logo,
            woke: false,
        }
    }
}

impl Screen for SleepScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        // The page stays: nothing is drawn over it.
        (self.show == PAGE).then_some(Rect::new(0, 0, 0, 0))
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        if self.show == PAGE {
            return;
        }
        let display = env.device.display;
        let screen = Rect::new(0, 0, display.width, display.height);
        if !self.widgets.is_empty() {
            for (widget, area) in &self.widgets {
                widget.draw(canvas, env, *area);
            }
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
    }

    /// A cover or a picture: in grays where the panel can.
    fn grays(&self) -> bool {
        self.show != PAGE && self.image.is_some() && self.widgets.is_empty()
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

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        if *event == events::RESUME {
            // Awake: gone, if on top. A screen that opened as the device
            // woke (a lock) is above it now; this one goes when it shows
            // again.
            self.woke = true;
            cx.command(Command::Back);
            if self.show != PAGE {
                cx.request_refresh(RefreshMode::Full);
            }
        }
    }
}

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
