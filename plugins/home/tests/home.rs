//! The home screen on a real kernel: the grid stays on the screen with no
//! margin, pages are edited and added where their settings are, the
//! layout chooser fits the screen and offers only layouts whose widgets
//! are there, and widgets are made anew only when what makes them
//! changed.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat, Point,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::widgets::{Widget, WidgetSpec};
use paperpaw_sdk::{
    Canvas, Env, Id, Manifest, MemoryStorage, Params, Plugin, Rect, RegisterError, Registrar,
    SDK_VERSION, SettingSpec, Version,
};

/// A device with `touch` or buttons alone, `width` pixels wide.
const fn device(touch: bool, width: u16) -> DeviceInfo {
    DeviceInfo {
        id: "test",
        name: "Test device",
        display: DisplayInfo {
            width,
            height: 800,
            dpi: 220,
            format: PixelFormat::Gray4,
            rotation: Rotation::Deg0,
            color: None,
        },
        input: InputCaps {
            buttons: true,
            touch,
            pen: false,
            keys: &[],
        },
        memory: MemoryClass::Small,
    }
}

/// How often the probe widget was made.
static MADE: AtomicUsize = AtomicUsize::new(0);

/// A plugin with a widget that counts how often it is made, and a
/// setting of its own that its widget does not have.
struct Probe;

const PROBE: Id = Id::new_static("probe.widget");
const OTHER: Setting<bool> = Setting::new("probe.other");
const OWN: Setting<bool> = Setting::new("probe.widget.own");

struct ProbeWidget;

impl Widget for ProbeWidget {
    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        40
    }

    fn draw(&self, _: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}
}

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: Id::new_static("probe"),
            name: Cow::Borrowed("Probe"),
            description: Cow::Borrowed("Counts widgets made"),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_widget(WidgetSpec::new(PROBE, "probe.widget", |_| {
            MADE.fetch_add(1, Ordering::SeqCst);
            Box::new(ProbeWidget)
        }))?;
        r.add_setting(SettingSpec::switch(&OTHER, "probe.other", false))?;
        r.add_setting(SettingSpec::switch(&OWN, "probe.own", false))
    }
}

struct Home {
    kernel: Kernel,
    now: u64,
}

impl Home {
    /// The home screen on a phone-sized device with `touch` or buttons
    /// alone, with `settings` in the user's config (under `[home]`, other
    /// sections after their names).
    fn new(touch: bool, settings: &str) -> Self {
        Self::on(device(touch, 480), settings)
    }

    /// The same on `device`.
    fn on(device: DeviceInfo, settings: &str) -> Self {
        let config = format!("[core]\nlanguage = en\n[home]\ngesture-tips = false\n{settings}");
        let mut kernel = Kernel::builder(device)
            .storage(MemoryStorage::new())
            .user_config(&config)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(Probe)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut home = Self { kernel, now: 0 };
        home.settle(effects);
        home
    }

    /// Lets timers run until nothing is due within a second.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..1000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn set(&mut self, id: &str, value: &str) {
        self.now += 100;
        let effects = self
            .kernel
            .set_setting(Instant(self.now), id, value)
            .unwrap();
        self.settle(effects);
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn open(&mut self, screen: &'static str, params: Params) {
        self.now += 100;
        let effects = self
            .kernel
            .open_screen(Instant(self.now), Id::new_static(screen), params);
        self.settle(effects);
    }

    /// What the screen shows: each part's label and place.
    fn shown(&mut self) -> Vec<(String, Rect)> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| (p.label, p.area))
            .collect()
    }

    /// A tap on the part of the screen that says `label`.
    fn tap_on(&mut self, label: &str) {
        let area = self
            .shown()
            .into_iter()
            .find(|(l, _)| l.contains(label))
            .unwrap_or_else(|| panic!("nothing says {label}"))
            .1;
        let at = Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        );
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 50;
            let event = InputEvent::Pointer(PointerEvent {
                id: 0,
                phase,
                point: at,
                tool: PointerTool::Finger,
                pressure: 1000,
            });
            let effects = self.kernel.handle(Instant(self.now), event);
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }

    fn says(&mut self, text: &str) -> bool {
        self.shown().iter().any(|(label, _)| label.contains(text))
    }
}

/// Every part on the screen lies on it.
fn on_screen(home: &Home, shown: &[(String, Rect)]) {
    let display = home.kernel.device().display;
    for (label, area) in shown {
        assert!(
            area.x + area.width <= display.width && area.y + area.height <= display.height,
            "{label} at {area:?}"
        );
    }
}

#[test]
fn without_margins_the_grid_stays_on_the_screen() {
    let mut home = Home::new(
        true,
        "page-1 = tool:core.settings 0/0 1x1, tool:core.settings 3/0 1x1\n\
         [core]\nui-margin = 0\n",
    );
    let shown = home.shown();
    on_screen(&home, &shown);
    let tools: Vec<_> = shown.iter().filter(|(l, _)| l == "Settings").collect();
    assert_eq!(tools.len(), 2, "{shown:?}");
    // In the first column and the last of four.
    let (left, right) = (tools[0].1, tools[1].1);
    assert!(left.x + left.width <= 120, "{left:?}");
    assert!(right.x >= 360, "{right:?}");
}

#[test]
fn a_page_is_edited_where_its_setting_is() {
    // The second page empty: the third shows second, the fourth third.
    let mut home = Home::new(
        true,
        "page-2 =\n\
         page-3 = tool:core.settings 0/0 1x1\n\
         page-4 = tool:core.library 0/0 1x1\n",
    );
    let page_3 = Params::new().with(params::ID, "home.page-3");
    home.open("home.dashboard", page_3);
    assert!(home.says("Settings"));
    assert!(!home.says("Library"));
}

#[test]
fn an_added_page_shows_until_it_is_removed() {
    let mut home = Home::new(true, "");
    home.open("home.add-widget", Params::new().with("page", "0"));
    home.tap_on("Add a page");
    let settings = home.kernel.settings();
    assert_eq!(settings.text("home.page-2"), "home.tools");
    assert_eq!(settings.text("home.page-3"), paperpaw_sdk::widgets::SPACE);
}

#[test]
fn the_layout_chooser_fits_a_screen_without_touch() {
    // Wide enough for what each layout is to fit on a line.
    let mut home = Home::on(device(false, 1200), "");
    home.open("home.layouts", Params::new());
    let shown = home.shown();
    assert!(
        shown.iter().any(|(l, _)| l.contains("as PaperPaw comes")),
        "{shown:?}"
    );
    on_screen(&home, &shown);
}

#[test]
fn layouts_with_widgets_not_there_are_not_offered() {
    let mut home = Home::new(true, "");
    home.open("home.layouts", Params::new());
    let mut seen = Vec::new();
    for _ in 0..8 {
        let shown = home.shown();
        for name in ["Calm", "Covers", "At a glance", "Clock", "Minimal"] {
            if shown.iter().any(|(l, _)| l == name) && !seen.contains(&name) {
                seen.push(name);
            }
        }
        home.run(actions::RIGHT);
    }
    // Statistics, the file manager and the widget pack are not there.
    assert_eq!(seen, ["Calm", "Minimal"]);
}

#[test]
fn widgets_are_made_anew_only_when_what_makes_them_changes() {
    let mut home = Home::new(true, "page-1 = probe.widget 0/0 4x1\n");
    home.shown();
    let made = MADE.load(Ordering::SeqCst);
    assert_eq!(made, 1, "made once as the home screen showed");
    // Another plugin's setting: read anew, not made anew.
    home.set(OTHER.as_str(), "true");
    home.shown();
    assert_eq!(MADE.load(Ordering::SeqCst), made);
    // The widget's own setting, the look of all: made anew.
    home.set(OWN.as_str(), "true");
    home.shown();
    assert_eq!(MADE.load(Ordering::SeqCst), made + 1);
    home.set("core.ui-lines", "5");
    home.shown();
    assert_eq!(MADE.load(Ordering::SeqCst), made + 2);
}

impl Home {
    /// The gray of each pixel of the screen, drawn anew.
    fn pixels(&mut self) -> (u16, Vec<u8>) {
        let display = self.kernel.device().display;
        let (width, height) = (display.width, display.height);
        let mut gray = vec![255u8; usize::from(width) * usize::from(height)];
        self.kernel.render(&mut |top, view| {
            for y in 0..view.height {
                for x in 0..view.width.min(width) {
                    let row = usize::from(top + y);
                    if row < usize::from(height) {
                        gray[row * usize::from(width) + usize::from(x)] = view.pixel(x, y).luma();
                    }
                }
            }
        });
        (width, gray)
    }
}

/// The rows from `top` to `bottom` with ink between `left` and `right`.
fn inked_rows(
    width: u16,
    gray: &[u8],
    (left, right): (u16, u16),
    top: u16,
    bottom: u16,
) -> Vec<u16> {
    (top..bottom)
        .filter(|y| {
            (left..right).any(|x| gray[usize::from(*y) * usize::from(width) + usize::from(x)] < 128)
        })
        .collect()
}

/// A tool's name sits clearly nearer its own icon than the tool below
/// it, on a short screen of buttons alone and on a tall touch screen,
/// in cells as flat as the X4's and as tall as a large screen's.
#[test]
fn a_tools_name_belongs_to_its_own_icon() {
    for (touch, rows) in [(false, 0), (true, 0), (true, 8), (false, 12)] {
        let mut home = Home::new(
            touch,
            &format!(
                "page-1 = tool:core.settings 0/0 1x1, tool:core.settings 0/1 1x1\n\
                 rows = {rows}\nstatus-bar = false\n"
            ),
        );
        let (width, gray) = home.pixels();
        let shown = home.shown();
        let names: Vec<Rect> = shown
            .iter()
            .filter(|(l, _)| l == "Settings")
            .map(|(_, r)| *r)
            .collect();
        assert_eq!(names.len(), 2, "{shown:?}");
        let column = (names[0].x, names[0].x + names[0].width);
        // The first tool's icon: the ink above its name (whose round
        // letters reach a pixel or two above its capitals).
        let above = inked_rows(width, &gray, column, 0, names[0].y.saturating_sub(2));
        let icon_bottom = *above.last().unwrap();
        // Its name's ink, and the second tool's icon below it.
        let name = inked_rows(
            width,
            &gray,
            column,
            names[0].y,
            names[0].y + names[0].height,
        );
        let name_top = name[0];
        let below = inked_rows(
            width,
            &gray,
            column,
            names[0].y + names[0].height,
            names[1].y,
        );
        let (name_bottom, next_icon) = (*name.last().unwrap(), below[0]);
        let inside = name_top - icon_bottom;
        let between = next_icon - name_bottom;
        assert!(
            inside * 2 <= between,
            "rows {rows}, touch {touch}: name {inside} px below its icon, {between} px above the next"
        );
    }
}
