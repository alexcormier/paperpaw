//! Input to actions: buttons and fingers recognized, the edges' swipes,
//! the key map resolved, presses that came while a frame was not shown
//! yet dropped.

#[allow(unused_imports)]
use super::*;

/// Where input is: gestures being recognized, the edges' swipes, presses
/// dropped while a frame was not shown yet.
#[derive(Debug)]
pub(crate) struct InputState {
    pub(crate) recognizer: Recognizer,
    /// Pointer ids captured by the active screen.
    pub(crate) captured: Vec<u8>,
    /// A finger that came down at an edge, followed by the kernel for a
    /// swipe in from it (see [`keymap::InputKey::Edge`]).
    pub(crate) edge_touch: Option<EdgeTouch>,
    /// A swipe in from an edge on a screen with its own gestures, waiting
    /// for the second one that applies it: the side, and until when.
    pub(crate) edge_pending: Option<(Direction, Instant)>,
    pub(crate) settling: bool,
    /// Presses made before this, while another screen was coming up, are
    /// dropped (see [`Kernel::shown`]), with their releases.
    pub(crate) stale_before: Option<Instant>,
    /// When the last two frames showed: presses made before the earlier
    /// one are more than a frame behind and dropped (see
    /// [`Kernel::shown`]).
    pub(crate) shown_at: [Option<Instant>; 2],
    pub(crate) dropped_buttons: Vec<&'static str>,
    pub(crate) dropped_pointers: Vec<u8>,
    /// Page buttons pressed while they were swapped (see
    /// `Kernel::page_buttons_swapped`): let go, they stay swapped.
    pub(crate) swapped: Vec<&'static str>,
}

impl Kernel {
    pub(crate) fn thresholds(&self) -> Thresholds {
        let metrics = Metrics::new(self.shared.device.display.dpi);
        Thresholds {
            long_press_ms: self
                .shared
                .settings
                .int(core_settings::LONG_PRESS_MS.as_str())
                .max(0) as u64,
            tap_slop: metrics.mm(3),
            swipe_distance: metrics.mm(8),
        }
    }

    pub(crate) fn pointer(&mut self, event: PointerEvent) {
        // Swipes in from an edge come before the screen sees the finger.
        // Held still there, it becomes a long press all the same.
        if let Some(touch) = self.input.edge_touch.filter(|t| t.id == event.id) {
            match event.phase {
                PointerPhase::Up => {
                    self.input.edge_touch = None;
                    let thresholds = self.thresholds();
                    if !self
                        .input
                        .recognizer
                        .lift_edge(event.id, self.now, &thresholds)
                    {
                        self.edge_lifted(touch, event.point);
                    }
                }
                PointerPhase::Cancel => {
                    self.input.edge_touch = None;
                    self.input.recognizer.forget_pointer(event.id);
                }
                PointerPhase::Move => {
                    let thresholds = self.thresholds();
                    self.input.recognizer.pointer(&event, self.now, &thresholds);
                }
                PointerPhase::Down => {}
            }
            return;
        }
        // A screen may take a finger before the edges do: what it takes
        // is meant for it (a grip that moves a row, a handle), as apps on
        // phones keep such places from the system's gestures.
        let captured = self.input.captured.contains(&event.id);
        if captured || event.phase == PointerPhase::Down {
            let handled = self.with_top(|screen, cx| screen.on_pointer(&event, cx));
            if captured {
                if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                    self.input.captured.retain(|&id| id != event.id);
                }
                return;
            }
            if handled == Handled::Yes {
                self.input.captured.push(event.id);
                return;
            }
        }
        if event.phase == PointerPhase::Down
            && let Some(side) = self.edge_at(event.point)
        {
            self.input.edge_touch = Some(EdgeTouch {
                id: event.id,
                side,
                start: event.point,
                tool: event.tool,
            });
            let edge = match side {
                Direction::Up | Direction::Down => input::Edge::End,
                Direction::Left | Direction::Right => input::Edge::Side,
            };
            self.input.recognizer.follow_edge(&event, self.now, edge);
            return;
        }
        let thresholds = self.thresholds();
        if let Some(recognized) = self.input.recognizer.pointer(&event, self.now, &thresholds) {
            self.recognized(recognized);
        }
    }

    /// How the screen shown takes gestures (`gestures.policy`).
    pub(crate) fn policy(&self) -> keymap::Policy {
        keymap::Policy::of(self.resolve(&InputKey::Policy))
    }

    /// The edge a finger coming down at `point` may swipe in from: one
    /// with something bound, where the screen lets edges apply.
    pub(crate) fn edge_at(&self, point: Point) -> Option<Direction> {
        if self.policy() == keymap::Policy::Off
            || self
                .stack
                .last()
                .is_some_and(|top| top.screen.holds_edges())
        {
            return None;
        }
        let display = self.shared.device.display;
        // Wide at the top and the bottom: on Android the outermost strip is
        // the system's (its notifications, its home), a swipe begun just
        // inside it is ours. Narrow at the sides, where pages are tapped.
        let metrics = Metrics::new(display.dpi);
        let mm = |id: &Id| {
            let value = self.shared.settings.int(id.as_str()).clamp(0, 50) as u32;
            metrics.mm(value) as i32
        };
        let (across, along) = (
            mm(&core_settings::EDGE_ENDS),
            mm(&core_settings::EDGE_SIDES),
        );
        let (w, h) = (i32::from(display.width), i32::from(display.height));
        let side = if point.y < across {
            Direction::Up
        } else if point.y >= h - across {
            Direction::Down
        } else if point.x < along {
            Direction::Left
        } else if point.x >= w - along {
            Direction::Right
        } else {
            return None;
        };
        let bound = self
            .resolve(&InputKey::Edge(side))
            .is_some_and(|action| *action != paperpaw_sdk::builtin::actions::NOTHING);
        bound.then_some(side)
    }

    /// A finger that came down at an edge was lifted at `end`: a swipe in
    /// from the edge applies what it is bound to (on screens with their
    /// own gestures, a second one within a few seconds); anything else
    /// is what it would have been elsewhere, a tap or a swipe.
    pub(crate) fn edge_lifted(&mut self, touch: EdgeTouch, end: Point) {
        let (start, thresholds) = (touch.start, self.thresholds());
        let (dx, dy) = (end.x - start.x, end.y - start.y);
        let (inward, across) = match touch.side {
            Direction::Up => (dy, dx),
            Direction::Down => (-dy, dx),
            Direction::Left => (dx, dy),
            Direction::Right => (-dx, dy),
        };
        if inward >= thresholds.swipe_distance as i32 && across.abs() < inward {
            if self.policy() == keymap::Policy::Own {
                let again = self
                    .input
                    .edge_pending
                    .take()
                    .is_some_and(|(side, until)| side == touch.side && self.now < until);
                if !again {
                    self.input.edge_pending = Some((touch.side, self.now.add_millis(3000)));
                    let text = self.shared.translator.tr("core.edge-again").to_string();
                    self.output.commands.push(Command::Notify(text));
                    return;
                }
            }
            return self.route_key(InputKey::Edge(touch.side));
        }
        let moved = input::distance(start, end);
        let kind = if moved <= thresholds.tap_slop {
            GestureKind::Tap
        } else if moved >= thresholds.swipe_distance {
            GestureKind::Swipe(input::direction(start, end))
        } else {
            return;
        };
        self.recognized(Recognized::Gesture(Gesture {
            kind,
            start,
            end,
            tool: touch.tool,
        }));
    }

    pub(crate) fn recognized(&mut self, recognized: Recognized) {
        match recognized {
            Recognized::Button { name, long } => {
                self.hint.pressed = self.hint.buttons.iter().position(|b| *b == name);
                if self.hint.pressed.is_some() {
                    self.show_hints();
                }
                let name = Cow::Borrowed(name);
                self.route_key(if long {
                    InputKey::LongButton(name)
                } else {
                    InputKey::Button(name)
                });
            }
            Recognized::Gesture(gesture) => {
                if gesture.kind == GestureKind::Tap
                    && let Some(row) = self.hint_row()
                    && row.contains(gesture.start)
                {
                    let count = self.hint.buttons.len() as u32;
                    let x = u32::try_from(gesture.start.x).unwrap_or(0);
                    let cell = (x * count / u32::from(row.width).max(1)) as usize;
                    if let Some(button) = self.hint.buttons.get(cell) {
                        self.hint.pressed = Some(cell);
                        self.route_key(InputKey::Button(Cow::Borrowed(button)));
                    }
                    return;
                }
                let overlay = self
                    .stack
                    .len()
                    .checked_sub(1)
                    .and_then(|top| self.overlay(top));
                if let Some(region) = overlay
                    && gesture.kind == GestureKind::Tap
                    && !region.contains(gesture.start)
                {
                    self.output.commands.push(Command::Back);
                    return;
                }
                if self.with_top(|screen, cx| screen.on_gesture(gesture, cx)) == Handled::Yes {
                    return;
                }
                // Screens with gestures of their own: none from the key map.
                if self.policy() != keymap::Policy::Standard {
                    return;
                }
                let display = self.shared.device.display;
                let zone = Zone::of(gesture.start, display.width, display.height);
                let key = match gesture.kind {
                    GestureKind::Tap => InputKey::Tap(zone),
                    GestureKind::LongPress => InputKey::LongTap(zone),
                    GestureKind::Swipe(direction) => InputKey::Swipe(direction),
                    GestureKind::Pinch { out } => InputKey::Pinch(out),
                    _ => return,
                };
                self.route_key(key);
            }
        }
    }

    /// Resolves an input through the key map layers of the active screen,
    /// its slot and the global layer.
    pub(crate) fn route_key(&mut self, key: InputKey) {
        if let Some(action) = self.resolve(&key).cloned() {
            self.dispatch(action);
        }
    }

    /// What `key` is bound to in the key map layers of the active screen,
    /// its slot and the global layer.
    /// It runs many times in each call (the row of hints asks for every
    /// button in it), so it allocates nothing.
    pub(crate) fn resolve(&self, key: &InputKey) -> Option<&Id> {
        let mut layers = [GLOBAL_KEYMAP_LAYER; 3];
        let mut count = 0;
        if let Some(top) = self.stack.last() {
            layers[0] = top.id.as_str();
            count = 1;
            if let Some(slot) = &top.slot {
                layers[1] = slot.as_str();
                count = 2;
            }
        }
        // The global layer last, already in place.
        self.shared.keymap.resolve(&layers[..=count], key)
    }

    pub(crate) fn dispatch(&mut self, action: Id) {
        if self.with_top(|screen, cx| screen.on_action(&action, cx)) == Handled::Yes {
            return;
        }
        match self.shared.actions.get(action.as_str()) {
            Some(spec) => {
                if let Some(handler) = &spec.handler {
                    let env = self.shared.env(self.now);
                    let _running = self.shared.meter.enter(action.namespace());
                    let from = self.output.commands.len();
                    handler(&env, &mut self.output.commands);
                    // Run by a rule, not the reader: nothing said over reading.
                    let (reader, now) = (self.shared.reader, self.now);
                    let top = self.stack.last().map(|s| s.id.namespace());
                    let out = &mut self.output.commands;
                    let owner = action.namespace();
                    work::quiet(reader, top, now, from, owner, out, &mut self.works);
                }
            }
            None => self.warn_runtime(format!("unknown action `{action}`")),
        }
    }

    /// The name the recognizer knows button `name` by while it is held:
    /// the other page button's where it was pressed swapped (see
    /// `Kernel::page_buttons_swapped`).
    fn held_as(&self, name: &'static str) -> &'static str {
        use paperpaw_platform::buttons::{PAGE_BACK, PAGE_FORWARD};
        match name {
            PAGE_FORWARD if self.input.swapped.contains(&name) => PAGE_BACK,
            PAGE_BACK if self.input.swapped.contains(&name) => PAGE_FORWARD,
            _ => name,
        }
    }

    /// Whether `event` belongs to a press dropped (see [`Kernel::shown`]).
    /// A button or a finger is down once at a time: its release ends the
    /// press of it held, the one taken or the one dropped, and a press
    /// taken ends one dropped before whose release was missed.
    pub(crate) fn stale(&mut self, now: Instant, event: &InputEvent) -> bool {
        let early = self.input.stale_before.is_some_and(|before| now < before)
            || self.input.shown_at[0].is_some_and(|before| now < before);
        match event {
            InputEvent::Button { name, state } => {
                let held = self.input.recognizer.holds(self.held_as(name));
                let dropped = self.input.dropped_buttons.iter().position(|b| b == name);
                let pressed = *state == ButtonState::Pressed;
                if let Some(at) = dropped
                    && (!pressed || !early)
                {
                    self.input.dropped_buttons.swap_remove(at);
                }
                // Pressed again while held: the press held goes on, its
                // release is still to come.
                if pressed && early && !held && dropped.is_none() {
                    self.input.dropped_buttons.push(name);
                }
                if pressed {
                    early
                } else {
                    !held && dropped.is_some()
                }
            }
            InputEvent::Pointer(p) if p.phase == PointerPhase::Down => {
                let dropped = self
                    .input
                    .dropped_pointers
                    .iter()
                    .position(|&id| id == p.id);
                if !early {
                    if let Some(at) = dropped {
                        self.input.dropped_pointers.swap_remove(at);
                    }
                    return false;
                }
                // Down again: the touch of that finger before is over.
                self.input.recognizer.forget_pointer(p.id);
                if self.input.edge_touch.is_some_and(|t| t.id == p.id) {
                    self.input.edge_touch = None;
                }
                if dropped.is_none() {
                    self.input.dropped_pointers.push(p.id);
                }
                true
            }
            InputEvent::Pointer(p) => match self
                .input
                .dropped_pointers
                .iter()
                .position(|&id| id == p.id)
            {
                Some(at) => {
                    if matches!(p.phase, PointerPhase::Up | PointerPhase::Cancel) {
                        self.input.dropped_pointers.remove(at);
                    }
                    true
                }
                None => false,
            },
            _ => false,
        }
    }
}
