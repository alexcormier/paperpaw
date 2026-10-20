//! Ready-made layouts of the home screen (PLANNING §12.1, C27): presets
//! of the kind [`KIND`] (see `paperpaw_sdk::presets`), five of the home
//! screen's own and any other plugin's (data plugins bring them in a
//! `presets.ini`). A layout sets the pages and how the screen looks (its
//! columns and rows, what shows above and below the grid); the rest of
//! these goes back to its default. The widgets it places are placements
//! of their own (`home.recent@1`), so their looks come with the layout
//! and leave the widget's other placements alone.
//!
//! The chooser is the home screen itself, each layout shown on it as it
//! would be (its values shown, not kept: `Command::PreviewSettings`),
//! with a sheet below, as low as can be so that the page keeps its
//! shape: the layout's name, a dot for each, what it is where that fits
//! on a line, and by touch *Use this layout* between two arrows (with
//! buttons, the row of hints says what confirm does). Left and right (or the page buttons, or a
//! swipe) go from layout to layout, confirm uses it, back leaves all as
//! it was. Where the layout kept is none of them, it comes first, as
//! *Your layout*. It is offered in the first-run setup, among what can be
//! added while editing the home screen, and as the action
//! `home.choose-layout`.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::presets::{self, Preset};
use paperpaw_sdk::setup::{SetupPage, SetupStepSpec};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets;
use paperpaw_sdk::widgets::grid::{self, What};
use paperpaw_sdk::{
    ActionSpec, Canvas, Color, Command, Env, Gesture, GestureKind, Id, Params, Point, Rect,
    RegisterError, Registrar, SettingValue,
};

use crate::pages::{self, MAX_PAGES};

/// The kind of presets that are layouts of the home screen.
pub const KIND: &str = "home.layout";
/// Opens the chooser.
pub const CHOOSE: Id = Id::new_static("home.choose-layout");
/// The chooser: the home screen showing each layout.
pub(crate) const SCREEN: Id = Id::new_static("home.layouts");

/// The home screen's own layouts: calm (as it comes), the books first, a
/// look at one's reading, a large clock, and the book alone. Each has the
/// tools on its second page; each fits a small screen with buttons as
/// well as a large one by touch.
static PRESETS: [Preset; 5] = [
    Preset::fixed(
        "home.calm",
        KIND,
        "home.layout.calm",
        "home.layout.calm.help",
        "home.page-1 = home.clock, home.continue, home.recent
home.page-2 = home.tools",
    ),
    Preset::fixed(
        "home.covers",
        KIND,
        "home.layout.covers",
        "home.layout.covers.help",
        "home.page-1 = home.continue@1 0/0 4x2, home.recent@1 0/2 4x3, \
tool:core.library 0/5 1x1, tool:stats.open 1/5 1x1, tool:file-manager.open 2/5 1x1, \
tool:core.settings 3/5 1x1
home.page-2 = home.tools
home.rows = 6
home.recent@1.count = 8
home.recent@1.heading = off",
    ),
    Preset::fixed(
        "home.dashboard",
        KIND,
        "home.layout.dashboard",
        "home.layout.dashboard.help",
        "home.page-1 = home.clock 0/0 4x1, stats.widget.goal@1 0/1 2x2, \
stats.widget.today@1 2/1 2x2, stats.widget.streak@1 0/3 4x1, home.continue@1 0/4 4x2
home.page-2 = home.tools
home.rows = 6
stats.widget.goal@1.frame = core.frame.line
stats.widget.today@1.frame = core.frame.line
stats.widget.streak@1.frame = core.frame.line
stats.widget.streak@1.size = core.size.small",
    ),
    Preset::fixed(
        "home.clock-first",
        KIND,
        "home.layout.clock",
        "home.layout.clock.help",
        "home.page-1 = widgets.analogue-clock@1 0/0 4x3, home.date@1 0/3 4x1, \
home.continue@1 0/4 4x2
home.page-2 = home.tools
home.rows = 6
home.status-bar = off",
    ),
    Preset::fixed(
        "home.minimal",
        KIND,
        "home.layout.minimal",
        "home.layout.minimal.help",
        "home.page-1 = home.continue@1 0/1 4x3
home.page-2 = home.tools
home.rows = 5
home.status-bar = off
home.footer.dots = off
home.continue@1.frame = core.frame.none",
    ),
];

/// The settings of the home screen's look a layout sets, or puts back to
/// their defaults where it leaves them out.
const LOOK: [&str; 11] = [
    "home.page-1",
    "home.page-2",
    "home.page-3",
    "home.page-4",
    "home.columns",
    "home.rows",
    "home.status-bar",
    "home.tool-labels",
    "home.footer.pencil",
    "home.footer.dots",
    "home.footer.items",
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_presets(&PRESETS)?;
    r.add_action(ActionSpec::new(CHOOSE, "home.layouts.title").handler(
        |_: &Env<'_>, out: &mut Vec<Command>| {
            out.push(Command::OpenScreen(SCREEN, Params::new()));
        },
    ))?;
    r.add_icon(CHOOSE, Cow::Borrowed("layout"))?;
    // In the first-run setup, after the practical steps.
    r.add_setup_step(SetupStepSpec {
        id: Id::new_static("home.setup.layout"),
        title: Cow::Borrowed("home.setup.layout"),
        help: Cow::Borrowed("home.setup.layout.help"),
        order: 65,
        page: SetupPage::Screen(SCREEN),
        applies: Some(Box::new(|env: &Env<'_>| offered(env).nth(1).is_some())),
    })?;
    Ok(())
}

/// The layouts offered: those whose widgets and tools are all there (a
/// layout that places a plugin's widgets is left out while the plugin is
/// not installed or switched off).
fn offered<'a>(env: &'a Env<'_>) -> impl Iterator<Item = &'a Preset> + 'a {
    env.settings
        .presets()
        .of_kind(KIND)
        .filter(move |preset| complete(env, preset))
}

/// Whether every widget and tool `preset` places on its pages is there.
fn complete(env: &Env<'_>, preset: &Preset) -> bool {
    (0..MAX_PAGES)
        .filter_map(|page| preset.value(pages::setting(page).as_str()))
        .flat_map(grid::parse)
        .all(|item| match &item.what {
            What::Widget(id) => {
                id == pages::TOOLS
                    || id == widgets::SPACE
                    || id == widgets::FILL
                    || env.widgets.has(id)
            }
            What::Tool(action) => env.actions.contains(action.as_str()),
            _ => true,
        })
}

/// The placements of widgets in a page's layout (`home.recent@1`).
fn placements(text: &str) -> impl Iterator<Item = String> {
    grid::parse(text)
        .into_iter()
        .filter_map(|item| match item.what {
            What::Widget(id) if id.contains('@') => Some(id),
            _ => None,
        })
}

/// What using `preset` makes of the settings: its values, and the look
/// of the home screen it leaves out back at its defaults, the values of
/// the placements on the pages kept and its own among them.
pub(crate) fn changes(env: &Env<'_>, preset: &Preset) -> Vec<(Id, Option<SettingValue>)> {
    let settings = env.settings;
    let mut covered: Vec<Id> = LOOK.iter().map(|id| Id::new_static(id)).collect();
    let mut placed: Vec<String> = Vec::new();
    for page in 0..MAX_PAGES {
        let id = pages::setting(page);
        if let Some(SettingValue::Text(kept)) = settings.global(id.as_str()) {
            placed.extend(placements(kept));
        }
        if let Some(text) = preset.value(id.as_str()) {
            placed.extend(placements(text));
        }
    }
    for placement in placed {
        for id in settings.placement_values(&placement) {
            if !covered.contains(&id) {
                covered.push(id);
            }
        }
    }
    preset.changes(settings, &covered)
}

/// What a tap on the sheet does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tap {
    Previous,
    Next,
    Use,
}

/// The chooser's state: the layouts offered and the one shown, and the
/// sheet below them.
#[derive(Debug)]
pub(crate) struct Chooser {
    /// The layouts by id; `None` first where the layout kept is none of
    /// them: it, as it is.
    entries: Vec<Option<Id>>,
    at: usize,
    /// Whether the sheet tells what each is: where every description
    /// fits on a line.
    described: bool,
    sheet: Rect,
    previous: Option<Rect>,
    next: Option<Rect>,
    /// *Use this layout*, by touch.
    choose: Option<Rect>,
}

impl Chooser {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let layouts: Vec<&Preset> = offered(env).collect();
        let kept = layouts
            .iter()
            .position(|p| presets::applied(env.settings, &changes(env, p)));
        let mut entries: Vec<Option<Id>> = layouts.iter().map(|p| Some(p.id.clone())).collect();
        let at = kept.unwrap_or_else(|| {
            entries.insert(0, None);
            0
        });
        let style = UiStyle::new(env);
        let width = f32::from(
            env.device
                .display
                .width
                .saturating_sub(2 * style.margin as u16),
        );
        let fits = |key: &str| {
            style
                .font(env)
                .is_some_and(|f| f.measure(style.small, env.i18n.tr(key)) <= width)
        };
        let described = fits("home.layout.own.help")
            && layouts
                .iter()
                .all(|p| p.description.is_empty() || fits(&p.description));
        Self {
            entries,
            at,
            described,
            sheet: Rect::default(),
            previous: None,
            next: None,
            choose: None,
        }
    }

    /// The layout shown; `None` for the one kept.
    fn preset<'a>(&self, env: &'a Env<'_>) -> Option<&'a Preset> {
        let id = self.entries.get(self.at)?.as_ref()?;
        env.settings.presets().get(id.as_str())
    }

    /// Shows the layout chosen, not kept yet.
    pub(crate) fn preview(&self, env: &Env<'_>) -> Command {
        let values = self
            .preset(env)
            .map(|preset| changes(env, preset))
            .unwrap_or_default();
        Command::PreviewSettings(values)
    }

    /// Keeps the layout shown and closes the chooser.
    pub(crate) fn choose(&self, env: &Env<'_>) -> Vec<Command> {
        let mut out = alloc::vec![Command::PreviewSettings(Vec::new())];
        if let Some(preset) = self.preset(env) {
            out.extend(presets::commands(env.settings, &changes(env, preset)));
        }
        out.push(Command::Back);
        out
    }

    /// Shows the next layout (the one before if not `forward`); `false`
    /// at either end.
    pub(crate) fn turn(&mut self, forward: bool) -> bool {
        let to = if forward {
            (self.at + 1 < self.entries.len()).then_some(self.at + 1)
        } else {
            self.at.checked_sub(1)
        };
        to.map(|to| self.at = to).is_some()
    }

    /// The sheet's height on this screen: a row for the name, a line for
    /// what it is where that fits, and by touch a row for the button.
    pub(crate) fn height(&self, env: &Env<'_>) -> u16 {
        let style = UiStyle::new(env);
        let gap = style.gap as u16;
        let help = if self.described {
            style
                .font(env)
                .map_or(0, |f| f.line_metrics(style.small).line_height as u16)
                + gap / 2
        } else {
            0
        };
        if !env.device.input.touch {
            return style.row as u16 + help + gap;
        }
        gap + style.row as u16 * 2 + help + gap
    }

    /// Places the sheet's parts in `sheet`.
    pub(crate) fn layout(&mut self, env: &Env<'_>, sheet: Rect) {
        let style = UiStyle::new(env);
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let row = style.row as u16;
        let y = (sheet.y + sheet.height).saturating_sub(gap + row);
        let width = sheet.width.saturating_sub(2 * margin);
        self.sheet = sheet;
        // By touch, the button between arrows; with buttons alone, left,
        // right and confirm do that, as the row of hints says.
        if env.device.input.touch {
            let previous = Rect::new(margin, y, row, row);
            let next = Rect::new(sheet.width.saturating_sub(margin + row), y, row, row);
            self.choose = Some(Rect::new(
                previous.x + row + gap,
                y,
                width.saturating_sub(2 * (row + gap)),
                row,
            ));
            self.previous = Some(previous);
            self.next = Some(next);
        } else {
            self.choose = None;
            self.previous = None;
            self.next = None;
        }
    }

    /// What a tap at `point` does, if anything.
    pub(crate) fn tap(&self, point: Point) -> Option<Tap> {
        if self.choose.is_some_and(|r| r.contains(point)) {
            Some(Tap::Use)
        } else if self.previous.is_some_and(|r| r.contains(point)) {
            Some(Tap::Previous)
        } else if self.next.is_some_and(|r| r.contains(point)) {
            Some(Tap::Next)
        } else {
            None
        }
    }

    /// Whether `point` is on the sheet.
    pub(crate) fn contains(&self, point: Point) -> bool {
        self.sheet.contains(point)
    }

    pub(crate) fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return;
        };
        let s = self.sheet;
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let width = s.width.saturating_sub(2 * margin);
        canvas.fill(s, Color::WHITE);
        canvas.fill(
            Rect::new(0, s.y, s.width, style.line.max(1) as u16),
            Color::BLACK,
        );
        let (title, help) = match self.preset(env) {
            Some(p) => (env.i18n.tr(&p.title), env.i18n.tr(&p.description)),
            None => (
                env.i18n.tr("home.layout.own"),
                env.i18n.tr("home.layout.own.help"),
            ),
        };
        // The name, and a dot for each layout at the right.
        let title_h = style.row as u16;
        let top = s.y + if self.choose.is_some() { gap } else { gap / 2 };
        let dot = (style.gap * 2 / 3).max(6);
        let spacing = dot * 2;
        let count = self.entries.len() as u32;
        let dots_w = dot * count + (spacing - dot) * count.saturating_sub(1);
        let dots_x = i32::from(margin + width) - dots_w as i32;
        let name_room = f32::from(width) - dots_w as f32 - f32::from(gap);
        let name = ui::fit(&font, style.title, title, name_room);
        let name_row = Rect::new(margin, top, width, title_h);
        ui::draw_label(
            canvas,
            &font,
            style.title,
            f32::from(margin),
            name_row,
            &name,
            Color::BLACK,
        );
        let y = i32::from(top) + (i32::from(title_h) - dot as i32) / 2;
        let mut x = dots_x;
        for index in 0..self.entries.len() {
            if index == self.at {
                canvas.fill_rect(x, y, dot, dot, Color::BLACK);
            } else {
                canvas.stroke_rect(x, y, dot, dot, style.line.max(1), Color::BLACK);
            }
            x += spacing as i32;
        }
        // What it is, where that fits on a line.
        if self.described {
            let small_h = font.line_metrics(style.small).line_height as u16;
            let row = Rect::new(margin, top + title_h, width, small_h);
            let text = ui::fit(&font, style.small, help, f32::from(width));
            ui::draw_label(
                canvas,
                &font,
                style.small,
                f32::from(margin),
                row,
                &text,
                style.faint(60),
            );
        }
        if let Some(choose) = self.choose {
            let label = env.i18n.tr("home.layouts.use");
            ui::draw_button(canvas, env, &style, choose, label, true, false);
        }
        let size = style.title;
        for (rect, icon, name, enabled) in [
            (
                self.previous,
                "chevron-left",
                "home.layouts.previous",
                self.at > 0,
            ),
            (
                self.next,
                "chevron-right",
                "home.layouts.next",
                self.at + 1 < self.entries.len(),
            ),
        ] {
            let Some(rect) = rect else {
                continue;
            };
            let ink = ui::draw_icon_button(canvas, &style, rect, env.i18n.tr(name), false, false);
            let ink = if enabled { ink } else { style.faint(170) };
            let w = ui::icon_width(env, icon, size);
            let x = f32::from(rect.x) + (f32::from(rect.width) - w) / 2.0;
            ui::draw_icon(canvas, env, icon, size, x, rect, ink);
        }
    }
}

/// What the chooser does with a gesture: taps on the sheet, swipes from
/// layout to layout; `None` where it does nothing.
pub(crate) fn gesture(chooser: &Chooser, gesture: Gesture) -> Option<Tap> {
    match gesture.kind {
        GestureKind::Tap => chooser.tap(gesture.start),
        GestureKind::Swipe(paperpaw_sdk::Direction::Left) => Some(Tap::Next),
        GestureKind::Swipe(paperpaw_sdk::Direction::Right) => Some(Tap::Previous),
        _ => None,
    }
}

/// What the chooser does with an action.
pub(crate) fn action(action: &Id) -> Option<Tap> {
    let a = action;
    if [
        actions::LEFT,
        actions::UP,
        actions::PREVIOUS,
        actions::PREV_PAGE,
    ]
    .contains(a)
    {
        Some(Tap::Previous)
    } else if [
        actions::RIGHT,
        actions::DOWN,
        actions::NEXT,
        actions::NEXT_PAGE,
    ]
    .contains(a)
    {
        Some(Tap::Next)
    } else if *a == actions::CONFIRM {
        Some(Tap::Use)
    } else {
        None
    }
}

/// Whether `event` asks the chooser to show the layout anew. Only the
/// chooser's own previews concern the home screen: values shown above it
/// (a slider moving in the quick panel) leave it alone, as rebuilding its
/// widgets reads the card.
pub(crate) fn shows_anew(event: &Id) -> bool {
    *event == events::PREVIEW_CHANGED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calm_layout_is_the_home_screen_as_it_comes() {
        for (page, default) in pages::DEFAULTS.iter().enumerate() {
            let id = pages::setting(page);
            assert_eq!(PRESETS[0].value(id.as_str()).unwrap_or(""), *default);
        }
        assert_eq!(PRESETS[0].values().count(), 2);
    }

    #[test]
    fn every_layout_fits_its_grid() {
        for preset in &PRESETS {
            let rows = preset
                .value("home.rows")
                .map_or(grid::MAX_ROWS, |r| r.parse().unwrap());
            let page = grid::parse(preset.value("home.page-1").unwrap());
            assert!(!page.is_empty(), "{}", preset.id);
            for (n, item) in page.iter().enumerate() {
                let Some(at) = item.at else {
                    continue;
                };
                assert!(at.fits(4, rows), "{}: {item:?}", preset.id);
                for other in &page[..n] {
                    assert!(
                        !other.at.is_some_and(|o| o.overlaps(&at)),
                        "{}: {item:?}",
                        preset.id
                    );
                }
            }
        }
    }
}
