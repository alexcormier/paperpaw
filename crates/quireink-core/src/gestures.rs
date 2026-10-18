//! Input to actions: buttons and fingers recognized, the edges' swipes,
//! the key map resolved, presses that came while a frame was not shown
//! yet dropped.

#[allow(unused_imports)]
use super::*;

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
        if let Some(touch) = self.edge_touch.filter(|t| t.id == event.id) {
            match event.phase {
                PointerPhase::Up => {
                    self.edge_touch = None;
                    self.edge_lifted(touch, event.point);
                }
                PointerPhase::Cancel => self.edge_touch = None,
                _ => {}
            }
            return;
        }
        // A screen may take a finger before the edges do: what it takes
        // is meant for it (a grip that moves a row, a handle), as apps on
        // phones keep such places from the system's gestures.
        let captured = self.captured.contains(&event.id);
        if captured || event.phase == PointerPhase::Down {
            let handled = self.with_top(|screen, cx| screen.on_pointer(&event, cx));
            if captured {
                if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                    self.captured.retain(|&id| id != event.id);
                }
                return;
            }
            if handled == Handled::Yes {
                self.captured.push(event.id);
                return;
            }
        }
        if event.phase == PointerPhase::Down
            && let Some(side) = self.edge_at(event.point)
        {
            self.edge_touch = Some(EdgeTouch {
                id: event.id,
                side,
                start: event.point,
                tool: event.tool,
            });
            return;
        }
        let thresholds = self.thresholds();
        if let Some(recognized) = self.recognizer.pointer(&event, self.now, &thresholds) {
            self.recognized(recognized);
        }
    }

    /// How the screen shown takes gestures (`gestures.policy`).
    pub(crate) fn policy(&self) -> keymap::Policy {
        keymap::Policy::of(self.resolve(&InputKey::Policy).as_ref())
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
            .is_some_and(|action| action != quireink_sdk::builtin::actions::NOTHING);
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
                    .edge_pending
                    .take()
                    .is_some_and(|(side, until)| side == touch.side && self.now < until);
                if !again {
                    self.edge_pending = Some((touch.side, self.now.add_millis(3000)));
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
        if let Some(action) = self.resolve(&key) {
            self.dispatch(action);
        }
    }

    /// What `key` is bound to in the key map layers of the active screen,
    /// its slot and the global layer.
    pub(crate) fn resolve(&self, key: &InputKey) -> Option<Id> {
        let top = self.stack.last();
        let mut layers: Vec<&str> = Vec::with_capacity(3);
        if let Some(top) = top {
            layers.push(top.id.as_str());
            if let Some(slot) = &top.slot {
                layers.push(slot.as_str());
            }
        }
        layers.push(GLOBAL_KEYMAP_LAYER);
        self.shared.keymap.resolve(&layers, key).cloned()
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
                    handler(&env, &mut self.output.commands);
                }
            }
            None => self.warn_runtime(format!("unknown action `{action}`")),
        }
    }

    /// Whether `event` belongs to a press dropped (see [`Kernel::shown`]).
    pub(crate) fn stale(&mut self, now: Instant, event: &InputEvent) -> bool {
        let early = self.stale_before.is_some_and(|before| now < before)
            || self.shown_at[0].is_some_and(|before| now < before);
        match event {
            InputEvent::Button {
                name,
                state: ButtonState::Pressed,
            } if early => {
                self.dropped_buttons.push(name);
                true
            }
            InputEvent::Button {
                name,
                state: ButtonState::Released,
            } => match self.dropped_buttons.iter().position(|b| b == name) {
                Some(at) => {
                    self.dropped_buttons.remove(at);
                    true
                }
                None => false,
            },
            InputEvent::Pointer(p) if p.phase == PointerPhase::Down && early => {
                self.dropped_pointers.push(p.id);
                true
            }
            InputEvent::Pointer(p) => match self.dropped_pointers.iter().position(|&id| id == p.id)
            {
                Some(at) => {
                    if matches!(p.phase, PointerPhase::Up | PointerPhase::Cancel) {
                        self.dropped_pointers.remove(at);
                    }
                    true
                }
                None => false,
            },
            _ => false,
        }
    }
}
