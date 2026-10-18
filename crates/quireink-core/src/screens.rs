//! The stack of screens: opening and closing them, overlays and the parts
//! of the screen they take, the notice, and calls into the screens.

#[allow(unused_imports)]
use super::*;

impl Kernel {
    pub(crate) fn open(&mut self, id: Id, slot: Option<Id>, params: &Params) {
        let Some(spec) = self.registry.screens.get(&id) else {
            return self.warn_runtime(format!("unknown screen `{id}`"));
        };
        // Made as the full screen it opens over, which an overlay shares;
        // a full screen that wants the other size is made again for it.
        // Made once, at the size it asked for: above the row of hints, or
        // all of the screen.
        let hinted = self.registry.hints_of(&id) == Hints::Shown;
        self.shared.device.display.height = self.content_height(hinted);
        let running = self.shared.meter.enter(id.namespace());
        let screen = (spec.factory)(&self.shared.env(self.now), params);
        drop(running);
        self.screens_changed = true;
        self.stack.push(ActiveScreen {
            id,
            slot,
            hinted,
            params: params.clone(),
            screen,
        });
        let top = self.stack.len() - 1;
        let overlay = self.overlay(top);
        self.screen_changed(self.changed_region(top), overlay.is_some());
    }

    /// Shows `text` with the screen bound to the notice slot until the
    /// notice time is over.
    pub(crate) fn notify(&mut self, text: String) {
        let Some(id) = self.registry.resolve_slot(slots::NOTICE.as_str()).cloned() else {
            return;
        };
        let Some(spec) = self.registry.screens.get(&id) else {
            return;
        };
        let env = self.shared.env(self.now);
        let params = Params::new().with(builtin_params::TEXT, text);
        let running = self.shared.meter.enter(id.namespace());
        let screen = (spec.factory)(&env, &params);
        drop(running);
        let seconds = self
            .shared
            .settings
            .int(core_settings::NOTICE_SECONDS.as_str())
            .clamp(1, 60) as u32;
        let region = screen.region(&env).unwrap_or(self.content());
        self.end_notice();
        self.notice = Some((screen, self.now.add_millis(u64::from(seconds) * 1000)));
        self.invalidate(region);
        self.output.request_refresh(RefreshMode::Fast);
    }

    /// Binds an input for the user (see [`Command::Bind`]).
    pub(crate) fn bind(&mut self, layer: &str, input: &str, action: Option<Id>) {
        let valid_layer = layer == GLOBAL_KEYMAP_LAYER || Id::parse(layer).is_ok();
        if !valid_layer || !keymap::is_input(input) {
            return self.warn_runtime(format!("cannot bind `{input}` in `{layer}`"));
        }
        let section = format!("keymap.{layer}");
        match &action {
            Some(action) => self.save_config(&section, input, action.as_str()),
            None => {
                let storage = self.shared.storage.as_ref();
                let source = read_config(storage).unwrap_or_default();
                let updated = quireink_config::remove(&source, &section, input);
                let _ = storage.write(CONFIG_FILE, updated.as_bytes());
            }
        }
        // The user's bindings as the file has them now.
        let source = read_config(self.shared.storage.as_ref()).unwrap_or_default();
        self.keys
            .sources
            .retain(|(origin, _)| *origin != Origin::User);
        self.keys.sources.push((Origin::User, Cow::Owned(source)));
        self.rebuild_keymap();
    }

    /// Takes the notice away, if one shows.
    pub(crate) fn end_notice(&mut self) {
        let Some((screen, _)) = self.notice.take() else {
            return;
        };
        let env = self.shared.env(self.now);
        let region = screen.region(&env).unwrap_or(self.content());
        self.invalidate(region);
        self.output.request_refresh(RefreshMode::Fast);
    }

    /// Closes the screens from `index` on, the top one first, telling
    /// each ([`Screen::on_close`]).
    pub(crate) fn close_from(&mut self, index: usize) {
        for i in (index..self.stack.len()).rev() {
            self.with_screen(i, |screen, cx| {
                screen.on_close(cx);
                Handled::Yes
            });
            self.stack.truncate(i);
            self.screens_changed = true;
        }
        self.sync_content();
    }

    /// Closes all screens above `index`.
    pub(crate) fn close_above(&mut self, index: usize) {
        let mut region: Option<Rect> = None;
        let mut overlays_only = true;
        for i in index + 1..self.stack.len() {
            let r = self.changed_region(i);
            overlays_only &= self.overlay(i).is_some();
            region = Some(region.map_or(r, |u: Rect| u.union(&r)));
        }
        self.close_from(index + 1);
        self.stop_for_closed_screens();
        if let Some(region) = region {
            self.screen_changed(region, overlays_only);
        }
    }

    /// The region of an overlay, `None` for full screens.
    pub(crate) fn overlay(&self, index: usize) -> Option<Rect> {
        let env = self.shared.env(self.now);
        self.stack.get(index)?.screen.region(&env)
    }

    /// The area a screen occupies.
    /// What opening or closing screen `index` changes: its region, or
    /// all of the screen where it washes out what lies below it
    /// ([`Screen::dims_below`](quireink_sdk::Screen::dims_below)).
    pub(crate) fn changed_region(&self, index: usize) -> Rect {
        if self.overlay(index).is_some() && self.stack[index].screen.dims_below() {
            self.content()
        } else {
            self.region_of(index)
        }
    }

    pub(crate) fn region_of(&self, index: usize) -> Rect {
        self.overlay(index).unwrap_or_else(|| self.content())
    }

    /// The part of the screen that screens have: all of it, or the part
    /// above the row of hints.
    pub(crate) fn content(&self) -> Rect {
        let display = self.shared.device.display;
        Rect::new(0, 0, display.width, display.height)
    }

    /// The height screens have: all of the screen's, or the part above
    /// the row of hints.
    pub(crate) fn content_height(&self, hinted: bool) -> u16 {
        let panel = self.framebuffer.bounds().height;
        let row = if hinted { self.hint.height } else { 0 };
        panel - row.min(panel / 2)
    }

    /// Sizes the content for the screen on top, after screens closed; all
    /// of the screen is drawn again if that changed it.
    pub(crate) fn sync_content(&mut self) {
        self.hint.until = None;
        let before = self.shared.device.display.height;
        if let Some(hinted) = self.stack.last().map(|s| s.hinted) {
            self.shared.device.display.height = self.content_height(hinted);
        }
        if self.shared.device.display.height != before {
            self.hint.shown.clear();
            let panel = self.framebuffer.bounds();
            self.invalidate(panel);
        }
    }

    /// Makes every open screen anew for the size it has now, e.g. after
    /// the screen was resized or the row of hints changed.
    pub(crate) fn remake_screens(&mut self) {
        let now = self.now;
        for i in 0..self.stack.len() {
            let Some(spec) = self.registry.screens.get(&self.stack[i].id) else {
                continue;
            };
            self.shared.device.display.height = self.content_height(self.stack[i].hinted);
            let env = self.shared.env(now);
            let active = &mut self.stack[i];
            let _running = self.shared.meter.enter(active.id.namespace());
            active.screen = (spec.factory)(&env, &active.params);
        }
        self.hint.shown.clear();
        self.end_notice();
        self.sync_content();
        self.frame.unrendered = None;
        self.frame.rendered = None;
        let bounds = self.framebuffer.bounds();
        self.screen_changed(bounds, false);
    }

    /// Runs `f` with the screen at `index` and a context for it.
    pub(crate) fn with_screen(
        &mut self,
        index: usize,
        f: impl FnOnce(&mut dyn Screen, &mut Cx<'_, '_>) -> Handled,
    ) -> Handled {
        let earlier = mem::take(&mut self.output.redraw);
        let Some(active) = self.stack.get_mut(index) else {
            self.output.redraw = earlier;
            return Handled::No;
        };
        let env = self.shared.env(self.now);
        let running = self.shared.meter.enter(active.id.namespace());
        let mut cx = Cx::new(&env, &mut self.output);
        let handled = f(active.screen.as_mut(), &mut cx);
        drop(running);
        // `request_redraw` means the caller's own region. A screen below an
        // overlay is still partly visible (e.g. the page behind a menu that
        // changes a reading setting); one below a full screen is redrawn
        // when it is shown again.
        let requested = mem::replace(&mut self.output.redraw, earlier);
        if requested {
            let top = self.stack.len() - 1;
            if index == top {
                self.output.redraw = true;
            } else if (index + 1..=top).all(|i| self.overlay(i).is_some()) {
                let region = self.region_of(index);
                self.invalidate(region);
            }
        }
        handled
    }

    /// Runs `f` with the active screen and a context for it.
    pub(crate) fn with_top(
        &mut self,
        f: impl FnOnce(&mut dyn Screen, &mut Cx<'_, '_>) -> Handled,
    ) -> Handled {
        match self.stack.len().checked_sub(1) {
            Some(top) => self.with_screen(top, f),
            None => Handled::No,
        }
    }

    /// After the screen stack changed: redraw `region`, flashing for full
    /// screens and refreshing quickly when only overlays changed.
    pub(crate) fn screen_changed(&mut self, region: Rect, overlays_only: bool) {
        self.input.captured.clear();
        self.screen_timer = None;
        self.output.wake_at = None;
        self.invalidate(region);
        let flash = self
            .shared
            .settings
            .bool(core_settings::FLASH_SCREENS.as_str());
        self.output.request_refresh(if overlays_only || !flash {
            RefreshMode::Partial
        } else {
            RefreshMode::Full
        });
        self.with_top(|screen, cx| {
            screen.on_show(cx);
            Handled::Yes
        });
    }

    /// Stops what closed screens started: servers, sockets, speech.
    pub(crate) fn stop_for_closed_screens(&mut self) {
        let screens = self.stack.len();
        let out = &mut self.output.commands;
        self.io.servers.stop_for_closed_screens(screens, out);
        self.io.sockets.close_for_closed_screens(screens, out);
        self.io.speech.stop_for_closed_screens(screens);
    }
}

/// The part of `area` that `cover` leaves to be seen, as a rectangle:
/// `None` if it covers all of it; less the rows it covers at the top or
/// the bottom where it is as wide; else all of `area`.
pub(crate) fn uncovered(area: Rect, cover: Rect) -> Option<Rect> {
    let Some(common) = area.intersection(&cover) else {
        return Some(area);
    };
    if common == area {
        return None;
    }
    if common.x != area.x || common.width != area.width {
        return Some(area);
    }
    if common.y == area.y {
        let top = common.bottom() as u16;
        return Some(Rect::new(
            area.x,
            top,
            area.width,
            (area.bottom() - common.bottom()) as u16,
        ));
    }
    if common.bottom() == area.bottom() {
        return Some(Rect::new(area.x, area.y, area.width, common.y - area.y));
    }
    Some(area)
}

/// Identifies an open screen instance (by its address), e.g. the owner of
/// a settings scope.
pub(crate) fn screen_key(screen: &ActiveScreen) -> usize {
    core::ptr::from_ref::<dyn Screen>(screen.screen.as_ref()).cast::<()>() as usize
}
