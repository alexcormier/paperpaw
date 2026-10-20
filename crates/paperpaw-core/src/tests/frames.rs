//! What is drawn again and when: screens turned, slots opened anew,
//! overlays at the edge of the screen.

use super::*;
use paperpaw_sdk::Params;

const BASE: Id = Id::new_static("frames.base");
const PANEL: Id = Id::new_static("frames.panel");

/// A full screen on the home slot, and an overlay on the menu slot from
/// row `top` (its parameter) down, reaching past the screen with `past`.
struct FramesPlugin(Log);

struct Panel {
    region: Rect,
}

impl Screen for Panel {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        let r = self.region;
        canvas.fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            u32::from(r.height),
            Color::BLACK,
        );
    }

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

struct Base;

impl Screen for Base {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for FramesPlugin {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: Id::new_static("frames"),
            name: Cow::Borrowed("Frames"),
            description: Cow::Borrowed(""),
            version: Version::new(1, 0, 0),
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NOTHING,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_screen(ScreenSpec {
            id: BASE,
            title: Cow::Borrowed("base"),
            factory: Box::new(move |_, _| {
                log.borrow_mut().push(String::from("base made"));
                Box::new(Base)
            }),
        })?;
        r.bind_slot(slots::HOME, BASE, Priority::DEFAULT)?;
        r.add_screen(ScreenSpec {
            id: PANEL,
            title: Cow::Borrowed("panel"),
            factory: Box::new(|_, params| {
                let top = params.get("top").and_then(|t| t.parse().ok()).unwrap_or(45);
                let past = params.get("past").is_some();
                let height = if past { 90 } else { 90 - top };
                Box::new(Panel {
                    region: Rect::new(0, top, 90, height),
                })
            }),
        })?;
        r.bind_slot(slots::MENU, PANEL, Priority::DEFAULT)?;
        Ok(())
    }
}

fn frames_kernel(builder: crate::KernelBuilder, log: &Log) -> Kernel {
    let mut kernel = builder.plugin(FramesPlugin(log.clone())).build().unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel
}

fn panel(top: u16) -> Params {
    Params::new().with("top", alloc::format!("{top}"))
}

/// A slot opened again for something else shows only the new screen:
/// what the old one covered beyond it is drawn again.
#[test]
fn a_slot_opened_for_something_else_leaves_nothing_of_the_old_screen() {
    let log = log();
    let mut kernel = frames_kernel(Kernel::builder(DEVICE), &log);
    kernel.open_slot(Instant(10), slots::MENU, panel(30));
    render(&mut kernel);
    assert_eq!(kernel.frame().pixel(5, 40), Color::BLACK);
    kernel.open_slot(Instant(20), slots::MENU, panel(60));
    render(&mut kernel);
    assert_eq!(kernel.frame().pixel(5, 70), Color::BLACK);
    assert_eq!(kernel.frame().pixel(5, 40), Color::WHITE, "the old panel");
}

/// An overlay that says it reaches past the screen is refreshed only on
/// the screen.
#[test]
fn an_overlay_reaching_past_the_screen_refreshes_only_the_screen() {
    let log = log();
    let mut kernel = frames_kernel(Kernel::builder(DEVICE), &log);
    let params = panel(60).with("past", "1");
    let effects = kernel.open_slot(Instant(10), slots::MENU, params);
    let (region, _) = effects.refresh().unwrap();
    assert!(region.bottom() <= 90, "{region:?}");
    let effects = kernel.run_action(Instant(20), actions::BACK);
    let (region, _) = effects.refresh().unwrap();
    assert!(region.bottom() <= 90, "{region:?}");
}

/// Where the shell turns the screen, choosing another rotation makes the
/// screens anew once, when the shell resizes the kernel for it, not also
/// before; turned half way round (the same size) the row of hints goes
/// and comes back.
#[test]
fn turning_the_screen_makes_the_screens_once() {
    let log = log();
    let builder = Kernel::builder(DEVICE)
        .button_row(&["back", "confirm", "left", "right"])
        .rotation(true);
    let mut kernel = frames_kernel(builder, &log);
    let row = kernel.hint.height;
    assert!(row > 0);
    log.borrow_mut().clear();
    kernel
        .set_setting(Instant(10), "core.rotation", settings::ROTATIONS[2])
        .unwrap();
    assert!(log.borrow().is_empty(), "made before the shell turned it");
    kernel.resize(Instant(20), 90, 90);
    assert_eq!(*log.borrow(), ["base made"]);
    assert_eq!(kernel.hint.height, 0);
    log.borrow_mut().clear();
    kernel
        .set_setting(Instant(30), "core.rotation", settings::ROTATIONS[0])
        .unwrap();
    kernel.resize(Instant(40), 90, 90);
    assert_eq!(*log.borrow(), ["base made"]);
    assert_eq!(kernel.hint.height, row);
}
