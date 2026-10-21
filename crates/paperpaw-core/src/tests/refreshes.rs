//! Refreshes (SDK gap F1): why the panel is refreshed, verdicts of
//! filters (a mode, grays, later), a mode set exactly, a screen's own
//! mode when it opens.

use super::*;
use paperpaw_sdk::Params;
use paperpaw_sdk::filters::{Refresh, RefreshCause, RefreshFilterSpec, RefreshVerdict};

const PAGE: Id = Id::new_static("r.page");
const SHEET: Id = Id::new_static("r.sheet");
const STATUS: Id = Id::new_static("r.status");
const EXACT: Id = Id::new_static("r.exact");
const GRAYS: Id = Id::new_static("r.grays");

type Seen = Rc<RefCell<Vec<Refresh>>>;

/// A page: its status line changes (`r.status`), a change it sets the
/// mode of exactly (`r.exact`), a change that wants grays (`r.grays`).
struct Page;

impl Screen for Page {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == STATUS {
            cx.request_redraw_area(Rect::new(0, 0, 10, 5));
            cx.refresh_cause(RefreshCause::Status);
        } else if *action == EXACT {
            cx.request_redraw_area(Rect::new(0, 20, 10, 5));
            cx.request_refresh(RefreshMode::Full);
            cx.set_refresh(RefreshMode::Fast);
        } else if *action == GRAYS {
            cx.request_redraw_area(Rect::new(0, 40, 10, 5));
        } else {
            return Handled::No;
        }
        Handled::Yes
    }
}

struct Refreshing(Seen);

impl Plugin for Refreshing {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("r"), "Refreshing", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            PAGE,
            "page",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.add_screen(
            ScreenSpec::new(SHEET, "sheet", Box::new(|_, _| Box::new(Page)))
                .refresh_on_open(RefreshMode::Fast),
        )?;
        r.bind_slot(slots::HOME, PAGE, Priority::DEFAULT)?;
        let seen = self.0.clone();
        r.add_refresh_filter(RefreshFilterSpec::new(
            Id::new_static("r.filter"),
            "",
            Box::new(move |_, refresh| {
                seen.borrow_mut().push(refresh.clone());
                match refresh.cause {
                    // A status line changed: shown with what comes next.
                    RefreshCause::Status => RefreshVerdict::Defer,
                    _ if refresh.region.y == 40 => RefreshVerdict::Grays(true),
                    _ => RefreshVerdict::Keep,
                }
            }),
        ))
    }
}

fn refreshing() -> (Kernel, Seen) {
    let seen = Seen::default();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Refreshing(seen.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    seen.borrow_mut().clear();
    (kernel, seen)
}

/// Filters hear why the panel is refreshed: a screen opened, a status
/// line; a refresh deferred goes with the next one.
#[test]
fn a_deferred_refresh_goes_with_the_next() {
    let (mut kernel, seen) = refreshing();
    let effects = kernel.run_action(Instant(10), STATUS);
    assert_eq!(effects.refresh(), None);
    assert_eq!(seen.borrow()[0].cause, RefreshCause::Status);
    let effects = kernel.run_action(Instant(20), EXACT);
    let (region, mode) = effects.refresh().unwrap();
    assert_eq!(
        mode,
        RefreshMode::Fast,
        "set exactly, over the full requested"
    );
    assert!(
        region.contains(Point::new(1, 1)),
        "the status line: {region:?}"
    );
    assert!(region.contains(Point::new(1, 21)), "{region:?}");
    let effects = kernel.run_action(Instant(30), GRAYS);
    assert!(effects.grays, "a filter wanted grays");
}

/// A screen opened in the mode it asks for; the filters hear it was a
/// screen, on top, and how many partial refreshes were made since the
/// panel was last cleaned.
#[test]
fn a_screen_opens_in_its_own_mode() {
    let (mut kernel, seen) = refreshing();
    let effects = kernel.open_screen(Instant(10), SHEET, Params::new());
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Fast)
    );
    let refresh = seen.borrow()[0].clone();
    assert_eq!(refresh.cause, RefreshCause::Screens);
    assert_eq!(refresh.screen, Some(SHEET));
    assert!(!refresh.periodic);
    let effects = kernel.open_screen(Instant(20), PAGE, Params::new());
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Full)
    );
}

/// The periodic full refresh says it is one; a clearing refresh cleans
/// the panel as a full one does.
#[test]
fn the_periodic_full_refresh_says_so() {
    let (mut kernel, seen) = refreshing();
    kernel
        .set_setting(Instant(1), settings::FULL_REFRESH_INTERVAL.as_str(), "2")
        .unwrap();
    render(&mut kernel);
    kernel.frame.refresh.done(RefreshMode::Clear);
    seen.borrow_mut().clear();
    for at in [10, 20] {
        kernel.run_action(Instant(at), GRAYS);
        render(&mut kernel);
    }
    let seen = seen.borrow();
    assert_eq!(seen[0].since_full, 0);
    assert!(!seen[0].periodic);
    assert!(seen[1].periodic, "{:?}", seen[1]);
    assert_eq!(seen[1].mode, RefreshMode::Full);
}
