//! Bars that stay along an edge of the screen across the screens shown
//! (see [`paperpaw_sdk::chrome`]): the part of the screen they leave to a
//! screen, making and letting go of them, their input.

#[allow(unused_imports)]
use super::*;
use paperpaw_sdk::chrome::{ChromeSpec, Edge};
use paperpaw_sdk::{Gesture, StackEntry};

/// A bar a plugin added, made while it shows.
pub(crate) struct Chrome {
    pub(crate) spec: ChromeSpec,
    pub(crate) screen: Option<Box<dyn Screen>>,
}

impl fmt::Debug for Chrome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chrome")
            .field("spec", &self.spec)
            .field("made", &self.screen.is_some())
            .finish()
    }
}

/// `point` of the screen as seen from the corner of `area`.
pub(crate) fn local(point: Point, area: Rect) -> Point {
    Point::new(point.x - i32::from(area.x), point.y - i32::from(area.y))
}

/// `gesture` as seen from the corner of `area`.
pub(crate) fn local_gesture(mut gesture: Gesture, area: Rect) -> Gesture {
    gesture.start = local(gesture.start, area);
    gesture.end = local(gesture.end, area);
    gesture
}

/// `region` of `area`'s coordinates in the screen's.
pub(crate) fn on_screen(region: Rect, area: Rect) -> Rect {
    Rect::new(
        region.x.saturating_add(area.x),
        region.y.saturating_add(area.y),
        region.width,
        region.height,
    )
}

impl Kernel {
    /// Adds a bar a plugin contributed, where the device has what it
    /// needs: along its edge, outside the bars of lower priority.
    pub(crate) fn add_chrome(&mut self, spec: ChromeSpec) {
        if spec.need.is_some_and(|need| !need.met(self.shared.caps)) {
            return;
        }
        let at = self
            .chromes
            .iter()
            .position(|c| c.spec.priority < spec.priority)
            .unwrap_or(self.chromes.len());
        self.chromes.insert(at, Chrome { spec, screen: None });
    }

    /// The part of the screen a screen of `entry` gets (above the row of
    /// hints where `hinted`), what the bars shown over it leave; their
    /// strips into `strips`, if given.
    pub(crate) fn layout(
        &self,
        hinted: bool,
        entry: &StackEntry,
        mut strips: Option<&mut Vec<(usize, Rect)>>,
    ) -> Rect {
        let panel = self.framebuffer.bounds();
        let row = if hinted {
            self.hint.height.min(panel.height / 2)
        } else {
            0
        };
        let mut area = Rect::new(0, 0, panel.width, panel.height - row);
        let env = self.shared.env(self.now);
        for (index, chrome) in self.chromes.iter().enumerate() {
            if !(chrome.spec.shown)(&env, entry) {
                continue;
            }
            let size = (chrome.spec.size)(&env);
            let strip = match chrome.spec.edge {
                Edge::Top => {
                    let size = size.min(area.height / 2);
                    let strip = Rect::new(area.x, area.y, area.width, size);
                    area.y += size;
                    area.height -= size;
                    strip
                }
                Edge::Bottom => {
                    let size = size.min(area.height / 2);
                    area.height -= size;
                    Rect::new(area.x, area.y + area.height, area.width, size)
                }
                Edge::Left => {
                    let size = size.min(area.width / 2);
                    let strip = Rect::new(area.x, area.y, size, area.height);
                    area.x += size;
                    area.width -= size;
                    strip
                }
                _ => {
                    let size = size.min(area.width / 2);
                    area.width -= size;
                    Rect::new(area.x + area.width, area.y, size, area.height)
                }
            };
            if let Some(strips) = strips.as_deref_mut() {
                strips.push((index, strip));
            }
        }
        area
    }

    /// The bars that show now (those shown over the full screen shown;
    /// none while a screen guards the device): made where they come,
    /// let go where they go, drawn again.
    pub(crate) fn sync_chromes(&mut self) {
        let mut strips: Vec<(usize, Rect)> = Vec::new();
        let base = self.shown_base();
        if !self.chromes.is_empty()
            && self.guard().is_none()
            && let (Some(active), Some(entry)) = (self.stack.get(base), self.shared.stack.get(base))
        {
            self.layout(active.hinted, entry, Some(&mut strips));
        }
        for (index, chrome) in self.chromes.iter_mut().enumerate() {
            if !strips.iter().any(|(i, _)| *i == index) {
                chrome.screen = None;
            }
        }
        for &(index, strip) in &strips {
            if self.chromes[index].screen.is_some() {
                continue;
            }
            let base = self.shared.env(self.now);
            let device = base.device_within(strip);
            let env = base.within(&device, strip);
            let chrome = &mut self.chromes[index];
            let _running = self.shared.meter.enter(chrome.spec.id.namespace());
            chrome.screen = Some((chrome.spec.factory)(&env, &Params::new()));
        }
        // Drawn again, as what they show follows the screen shown.
        let old = mem::replace(&mut self.chrome_strips, strips);
        for (_, strip) in old.into_iter().chain(self.chrome_strips.clone()) {
            self.invalidate(strip);
        }
    }

    /// The bar whose strip holds `point`, and its strip.
    pub(crate) fn chrome_at(&self, point: Point) -> Option<(usize, Rect)> {
        self.chrome_strips
            .iter()
            .copied()
            .find(|(_, strip)| strip.contains(point))
    }

    /// Runs `f` with bar `index` and a context for it, in its strip's
    /// coordinates.
    pub(crate) fn with_chrome(
        &mut self,
        index: usize,
        f: impl FnOnce(&mut dyn Screen, &mut Cx<'_, '_>) -> Handled,
    ) -> Handled {
        let Some(&(_, strip)) = self.chrome_strips.iter().find(|(i, _)| *i == index) else {
            return Handled::No;
        };
        let running = self
            .shared
            .meter
            .enter(self.chromes[index].spec.id.namespace());
        let Some(screen) = self.chromes[index].screen.as_mut() else {
            return Handled::No;
        };
        let (redraw, area) = (
            mem::take(&mut self.output.redraw),
            self.output.redraw_area.take(),
        );
        let base = self.shared.env(self.now);
        let device = base.device_within(strip);
        let env = base.within(&device, strip);
        let mut cx = Cx::new(&env, &mut self.output);
        let handled = f(screen.as_mut(), &mut cx);
        drop(running);
        // What it asks to draw again is in its strip; a time it asks for
        // is not kept (bars draw what they show when shown).
        self.output.wake_at = None;
        if mem::replace(&mut self.output.redraw, redraw) {
            self.invalidate(strip);
        }
        if let Some(asked) = mem::replace(&mut self.output.redraw_area, area) {
            self.invalidate(on_screen(asked, strip));
        }
        handled
    }

    /// Every bar shown hears `event` (see [`Screen::on_event`]).
    pub(crate) fn chromes_hear(&mut self, event: &Id, params: &Params) {
        for (index, _) in self.chrome_strips.clone() {
            self.with_chrome(index, |screen, cx| {
                screen.on_event(event, params, cx);
                Handled::Yes
            });
        }
    }
}
