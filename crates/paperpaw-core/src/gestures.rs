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
    /// A press or tap held back while a second may still make it double
    /// (something is bound to that): its input, and until when.
    pub(crate) pending: Option<(InputKey, Instant)>,
    /// Where and when the last tap was, for a double tap.
    pub(crate) last_tap: Option<(Point, Instant)>,
    /// Plugins' recognizers of gestures.
    pub(crate) recognizers: Vec<PluginRecognizer>,
    /// The key map layer put in front of the slot's
    /// ([`Command::SetKeyLayer`]).
    pub(crate) key_layer: Option<Id>,
}

/// A plugin's recognizer of gestures (`gesture.<id>`).
pub(crate) struct PluginRecognizer {
    pub(crate) id: Id,
    pub(crate) recognizer: Box<dyn paperpaw_sdk::keys::GestureRecognizer>,
}

impl core::fmt::Debug for PluginRecognizer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("PluginRecognizer").field(&self.id).finish()
    }
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
            tap_slop: metrics.mm(self.mm_setting(&core_settings::TAP_SLOP, 1, 20)),
            swipe_distance: metrics.mm(self.mm_setting(&core_settings::SWIPE_DISTANCE, 2, 50)),
        }
    }

    /// Millimetres setting `setting` says, kept within `min` and `max`.
    fn mm_setting(&self, setting: &paperpaw_sdk::setting::Setting<i64>, min: i64, max: i64) -> u32 {
        self.shared.settings.read(setting).clamp(min, max) as u32
    }

    /// How long a second press or tap may come after the first to make
    /// it double, in milliseconds.
    fn double_ms(&self) -> u64 {
        let ms = self.shared.settings.read(&core_settings::DOUBLE_TAP_MS);
        ms.clamp(50, 2000) as u64
    }

    /// What a filter cancelled (see [`InputVerdict::Cancel`]): the button
    /// or finger no longer followed; a screen that took the finger is
    /// told it is gone.
    pub(crate) fn cancel_input(&mut self, event: &InputEvent) {
        match *event {
            InputEvent::Button { name, .. } => self.input.recognizer.forget_button(name),
            InputEvent::Pointer(p) => {
                self.input.recognizer.forget_pointer(p.id);
                if self.input.edge_touch.is_some_and(|t| t.id == p.id) {
                    self.input.edge_touch = None;
                }
                if self.input.captured.contains(&p.id) {
                    self.input.captured.retain(|&id| id != p.id);
                    let area = self.stack.last().map_or(Rect::default(), |top| top.area);
                    let gone = PointerEvent {
                        phase: PointerPhase::Cancel,
                        point: chrome::local(p.point, area),
                        ..p
                    };
                    self.with_top(|screen, cx| screen.on_pointer(&gone, cx));
                }
            }
            _ => {}
        }
    }

    /// Feeds pointer `event` to the plugins' recognizers, in the
    /// coordinates of the screen on top; what they recognize runs.
    fn feed_recognizers(&mut self, event: &PointerEvent) {
        if self.input.recognizers.is_empty() {
            return;
        }
        let area = self.stack.last().map_or(Rect::default(), |top| top.area);
        let seen = PointerEvent {
            point: chrome::local(event.point, area),
            ..*event
        };
        let (now, meter) = (self.now, self.shared.meter.clone());
        let made: Vec<Id> = self
            .input
            .recognizers
            .iter_mut()
            .filter_map(|r| {
                let _running = meter.enter(r.id.namespace());
                let recognized = r.recognizer.pointer(&seen, now);
                (recognized == paperpaw_sdk::keys::Recognition::Recognized).then(|| r.id.clone())
            })
            .collect();
        for id in made {
            self.route_key(InputKey::Gesture(id));
        }
    }

    /// Time for the plugins' recognizers that asked for it.
    pub(crate) fn tick_recognizers(&mut self) {
        let (now, meter) = (self.now, self.shared.meter.clone());
        let made: Vec<Id> = self
            .input
            .recognizers
            .iter_mut()
            .filter(|r| r.recognizer.next_tick().is_some_and(|at| at <= now))
            .filter_map(|r| {
                let _running = meter.enter(r.id.namespace());
                let recognized = r.recognizer.tick(now);
                (recognized == paperpaw_sdk::keys::Recognition::Recognized).then(|| r.id.clone())
            })
            .collect();
        for id in made {
            self.shared.reader = true;
            self.route_key(InputKey::Gesture(id));
        }
    }

    pub(crate) fn pointer(&mut self, event: PointerEvent) {
        self.feed_recognizers(&event);
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
        // A finger on a bar is the bar's: its gestures go to it.
        let on_bar = event.phase == PointerPhase::Down && self.chrome_at(event.point).is_some();
        if captured || (event.phase == PointerPhase::Down && !on_bar) {
            let area = self.stack.last().map_or(Rect::default(), |top| top.area);
            let seen = PointerEvent {
                point: chrome::local(event.point, area),
                ..event
            };
            let handled = self.with_top(|screen, cx| screen.on_pointer(&seen, cx));
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
            && !on_bar
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
        // Inside the part of the screen the screen on top has: not on a
        // bar or the row of hints.
        let display = self.stack.last().map_or(self.content(), |top| top.area);
        if !display.contains(point) {
            return None;
        }
        // Wide at the top and the bottom: on Android the outermost strip is
        // the system's (its notifications, its home), a swipe begun just
        // inside it is ours. Narrow at the sides, where pages are tapped.
        let metrics = Metrics::new(self.shared.device.display.dpi);
        let mm = |setting: &paperpaw_sdk::setting::Setting<i64>| {
            let value = self.shared.settings.read(setting).clamp(0, 50) as u32;
            metrics.mm(value) as i32
        };
        let (across, along) = (
            mm(&core_settings::EDGE_ENDS),
            mm(&core_settings::EDGE_SIDES),
        );
        let (left, top) = (i32::from(display.x), i32::from(display.y));
        let (right, bottom) = (display.right() as i32, display.bottom() as i32);
        let side = if point.y < top + across {
            Direction::Up
        } else if point.y >= bottom - across {
            Direction::Down
        } else if point.x < left + along {
            Direction::Left
        } else if point.x >= right - along {
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
        self.recognized(Recognized::Gesture(Gesture::new(
            kind, start, end, touch.tool,
        )));
    }

    pub(crate) fn recognized(&mut self, recognized: Recognized) {
        match recognized {
            Recognized::Button { name, long } => {
                self.hint.pressed = self.hint.buttons.iter().position(|b| *b == name);
                if self.hint.pressed.is_some() {
                    self.show_hints();
                }
                let name = Cow::Borrowed(name);
                if long {
                    self.route_key(InputKey::LongButton(name));
                } else {
                    let double = InputKey::DoubleButton(name.clone());
                    self.single_or_double(InputKey::Button(name), double);
                }
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
                if let Some((index, strip)) = self.chrome_at(gesture.start) {
                    let seen = chrome::local_gesture(gesture, strip);
                    self.with_chrome(index, |screen, cx| screen.on_gesture(seen, cx));
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
                    let top = self.stack.len() - 1;
                    match self.stack[top].screen.outside() {
                        paperpaw_sdk::Outside::Ignore => return,
                        paperpaw_sdk::Outside::PassThrough => {
                            let below = top.saturating_sub(1);
                            let seen = chrome::local_gesture(gesture, self.stack[below].area);
                            self.with_screen(below, |screen, cx| screen.on_gesture(seen, cx));
                            return;
                        }
                        _ => return self.close_top(paperpaw_sdk::CloseCause::TapOutside),
                    }
                }
                // In the coordinates of the screen on top.
                let area = self.stack.last().map_or(Rect::default(), |top| top.area);
                let gesture = self.doubled(chrome::local_gesture(gesture, area));
                if self.offer(|screen, cx| screen.on_gesture(gesture, cx)) == Handled::Yes {
                    return;
                }
                // A screen that knows no double taps gets a tap.
                if gesture.kind == GestureKind::DoubleTap {
                    let mut tap = gesture;
                    tap.kind = GestureKind::Tap;
                    if self.offer(|screen, cx| screen.on_gesture(tap, cx)) == Handled::Yes {
                        return;
                    }
                }
                // Screens with gestures of their own: none from the key map.
                if self.policy() != keymap::Policy::Standard {
                    return;
                }
                let display = self.shared.device.display;
                let zone = Zone::of(gesture.start, display.width, display.height);
                let long = gesture.kind == GestureKind::LongPress;
                let tapped = matches!(gesture.kind, GestureKind::Tap | GestureKind::DoubleTap);
                if (tapped || long)
                    && let Some(own) = self.own_zone(gesture.start, long)
                {
                    return self.route_key(own);
                }
                let key = match gesture.kind {
                    GestureKind::Tap | GestureKind::DoubleTap => {
                        return self
                            .single_or_double(InputKey::Tap(zone), InputKey::DoubleTap(zone));
                    }
                    GestureKind::LongPress => InputKey::LongTap(zone),
                    GestureKind::TwoFingerTap => InputKey::Tap2,
                    GestureKind::Swipe(direction) => InputKey::Swipe(direction),
                    GestureKind::Pinch { out } => InputKey::Pinch(out),
                    _ => return,
                };
                self.route_key(key);
            }
        }
    }

    /// Resolves an input through the key map layers shown (see
    /// [`Self::resolve`]); a press or tap held back for a second runs
    /// first, as it came first.
    pub(crate) fn route_key(&mut self, key: InputKey) {
        self.route_key_pending();
        if let Some(action) = self.resolve(&key).cloned() {
            self.dispatch(action);
        }
    }

    /// Runs the press or tap held back for a second, if any.
    pub(crate) fn route_key_pending(&mut self) {
        if let Some((key, _)) = self.input.pending.take()
            && let Some(action) = self.resolve(&key).cloned()
        {
            self.dispatch(action);
        }
    }

    /// Input `single` came. With something bound to `double`, it waits
    /// for a second ([`core_settings::DOUBLE_TAP_MS`]): the second makes
    /// `double`, none `single`. Else `single` runs at once.
    fn single_or_double(&mut self, single: InputKey, double: InputKey) {
        if self.resolve(&double).is_none() {
            return self.route_key(single);
        }
        match self.input.pending.take() {
            Some((first, until)) if first == single && self.now < until => self.route_key(double),
            other => {
                self.input.pending = other;
                self.route_key_pending();
                let until = self.now.add_millis(self.double_ms());
                self.input.pending = Some((single, until));
            }
        }
    }

    /// `gesture` as a double tap where it is a tap close in time and place
    /// to the tap before, for a screen on top that takes double taps.
    fn doubled(&mut self, mut gesture: Gesture) -> Gesture {
        let wanted = self
            .stack
            .last()
            .is_some_and(|top| top.screen.takes_double_taps());
        if gesture.kind != GestureKind::Tap || !wanted {
            return gesture;
        }
        let (window, near) = (self.double_ms(), self.thresholds().swipe_distance);
        let now = self.now;
        let double = self.input.last_tap.take().is_some_and(|(at, when)| {
            now.since(when) < window && input::distance(at, gesture.start) < near
        });
        if double {
            gesture.kind = GestureKind::DoubleTap;
        } else {
            self.input.last_tap = Some((gesture.start, now));
        }
        gesture
    }

    /// The tap (or long tap) at `point` in a zone of a layer's own
    /// (`[zones.<layer>]`), where something is bound to it.
    fn own_zone(&self, point: Point, long: bool) -> Option<InputKey> {
        let display = self.shared.device.display;
        let percent = |at: i32, of: u16| {
            let at = u64::try_from(at).unwrap_or(0);
            (at.saturating_mul(100) / u64::from(of).max(1)).min(100) as u8
        };
        let (layers, count) = self.layers();
        let (x, y) = (
            percent(point.x, display.width),
            percent(point.y, display.height),
        );
        let name = self.shared.keymap.zone_at(&layers[..count], x, y)?;
        let key = InputKey::ZoneTap(long, String::from(name));
        self.resolve(&key).is_some().then_some(key)
    }

    /// The key map layers shown, the most specific first: the screen on
    /// top, the layer a plugin put in front ([`Command::SetKeyLayer`]),
    /// the screen's slot, the global layer; and how many there are.
    fn layers(&self) -> ([&str; 4], usize) {
        let mut layers = [GLOBAL_KEYMAP_LAYER; 4];
        let mut count = 0;
        let top = self.stack.last();
        let named = [
            top.map(|top| top.id.as_str()),
            self.input.key_layer.as_ref().map(Id::as_str),
            top.and_then(|top| top.slot.as_ref()).map(Id::as_str),
        ];
        for layer in named.into_iter().flatten() {
            layers[count] = layer;
            count += 1;
        }
        // The global layer last, already in place.
        (layers, count + 1)
    }

    /// What `key` is bound to in the key map layers shown (see
    /// [`Self::layers`]).
    /// It runs many times in each call (the row of hints asks for every
    /// button in it), so it allocates nothing.
    pub(crate) fn resolve(&self, key: &InputKey) -> Option<&Id> {
        let (layers, count) = self.layers();
        self.shared.keymap.resolve(&layers[..count], key)
    }

    /// Offers input to the screen on top with `f`, and on to the screens
    /// below it while the one asked lets what it does not take pass
    /// ([`Screen::passes_through`]).
    pub(crate) fn offer(
        &mut self,
        f: impl Fn(&mut dyn Screen, &mut Cx<'_, '_>) -> Handled,
    ) -> Handled {
        let Some(mut index) = self.stack.len().checked_sub(1) else {
            return Handled::No;
        };
        loop {
            if self.with_screen(index, &f) == Handled::Yes {
                return Handled::Yes;
            }
            if index == 0 || !self.stack[index].screen.passes_through() {
                return Handled::No;
            }
            index -= 1;
        }
    }

    pub(crate) fn dispatch(&mut self, action: Id) {
        if self.offer(|screen, cx| screen.on_action(&action, cx)) == Handled::Yes {
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
                    // An action a screen guarding the device let pass (the
                    // lock lets sleep pass): what it opens may show above.
                    if self.guard().is_some() {
                        self.trusted.extend(from..self.output.commands.len());
                    }
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
