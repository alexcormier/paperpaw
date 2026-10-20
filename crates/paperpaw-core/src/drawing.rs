//! Drawing frames: what changed, in bands for the shell, in grays,
//! described for reading aloud and tests, captured as a screenshot; the
//! busy mark; the screen made anew for another size.

#[allow(unused_imports)]
use super::*;

/// What is drawn: the parts changed, the bands, large pictures shown, a
/// screenshot to save, the busy mark, how the panel refreshes.
#[derive(Debug)]
pub(crate) struct FrameState {
    /// The part of the screen to redraw at the end of the current call.
    pub(crate) pending: Option<Rect>,
    /// What changed but has not been passed to [`Kernel::render`] yet.
    pub(crate) unrendered: Option<Rect>,
    /// What [`Kernel::render`] passed last, for [`Kernel::rerender`].
    pub(crate) rendered: Option<Rect>,
    /// See [`KernelBuilder::row_align`].
    pub(crate) row_align: u16,
    /// The pixels of a band, where frames are drawn in bands
    /// ([`KernelBuilder::frame_rows`]): kept when the screen is turned.
    pub(crate) band: Option<u32>,
    /// The frame shown has large pictures (see [`PICTURES_PERCENT`]).
    pub(crate) pictures_shown: bool,
    /// A screenshot to save with the next frame drawn: its path.
    pub(crate) screenshot: Option<String>,
    /// The mark shown while slow work waits, and the hint cell it is.
    pub(crate) mark: Option<(Rect, Option<usize>)>,
    pub(crate) refresh: RefreshPolicy,
}

/// How long what a call opened, closed or ran takes (for the busy mark).
#[derive(Debug, Default)]
pub(crate) struct Timing {
    /// What the call opened, closed or ran, and when it began (the clock's
    /// microseconds), until the call is done.
    pub(crate) running: Option<(String, u64)>,
    /// How long that took the last time, in milliseconds.
    pub(crate) durations: Vec<(String, u32)>,
}

impl Kernel {
    /// Draws everything that changed since the last call and passes it to
    /// `out` as full-width rows of pixels, top to bottom: `out(top, rows)`
    /// with `top` the screen row of the first one. Call it after each
    /// [`start`](Kernel::start), [`handle`](Kernel::handle) or
    /// [`tick`](Kernel::tick) whose effects contain a refresh, then refresh
    /// the panel.
    /// Draws the whole screen again in four grays (two bits a pixel), in
    /// bands as [`Kernel::render`] passes them, for a shell that shows a
    /// frame's pictures in grays ([`Effects::grays`]): bands of as many
    /// bytes as its own, which goes meanwhile. `false` where there is no
    /// memory for it (nothing passed then).
    pub fn render_gray(&mut self, out: &mut dyn FnMut(u16, FrameView<'_>)) -> bool {
        use paperpaw_platform::PixelFormat;
        let format = self.framebuffer.format();
        let bounds = self.framebuffer.bounds();
        let rows = self.framebuffer.rows();
        let (inverted, saturation, kept, coverage) = (
            self.framebuffer.is_inverted(),
            self.framebuffer.saturation(),
            self.framebuffer.pictures_kept(),
            self.framebuffer.mono_coverage(),
        );
        let bits = u32::from(format.bits_per_pixel()).max(1);
        let align = self.frame.row_align;
        let gray_rows = (((u32::from(rows) * bits / 2) as u16) / align * align)
            .max(align)
            .min(bounds.height);
        let made = |width, height, rows, format| {
            Framebuffer::banded(width, height, rows, format).map(|mut framebuffer| {
                framebuffer.set_inverted(inverted);
                framebuffer.set_pictures_kept(kept);
                framebuffer.set_saturation(saturation);
                framebuffer.set_mono_coverage(coverage);
                framebuffer
            })
        };
        // The band goes first, the gray one taking its place.
        let Ok(none) = Framebuffer::new(1, 1, format) else {
            return false;
        };
        self.framebuffer = none;
        let drawn = match made(bounds.width, bounds.height, gray_rows, PixelFormat::Gray2) {
            Ok(gray) => {
                self.framebuffer = gray;
                self.frame.unrendered = Some(bounds);
                self.render(out);
                true
            }
            Err(_) => false,
        };
        if let Ok(none) = Framebuffer::new(1, 1, format) {
            self.framebuffer = none;
        }
        if let Ok(band) = made(bounds.width, bounds.height, rows, format) {
            self.framebuffer = band;
        }
        drawn
    }

    pub fn render(&mut self, out: &mut dyn FnMut(u16, FrameView<'_>)) {
        let Some(region) = self.frame.unrendered.take() else {
            return;
        };
        // The kernel's, but for the screens' own drawing.
        let _running = self.shared.meter.enter(meter::CORE);
        let bounds = self.framebuffer.bounds();
        // Rows are passed whole: drawn in bands, the rest of a row would be
        // left from another band. And aligned as the shell asks.
        let rows = self.framebuffer.rows();
        let region = if rows < bounds.height {
            Rect::new(0, region.y, bounds.width, region.height)
        } else {
            region
        };
        let align = self.frame.row_align;
        let first = region.y - region.y % align;
        let last = (region.bottom().div_ceil(u32::from(align)) * u32::from(align))
            .min(u32::from(bounds.height));
        let region = Rect::new(
            region.x,
            first,
            region.width,
            (last - u32::from(first)) as u16,
        );
        self.frame.rendered = Some(region);
        // A screenshot is written as the frame is drawn, a band at a time.
        let shot = self.frame.screenshot.take().filter(|_| region == bounds);
        let mut writer = shot.as_deref().and_then(|path| {
            paperpaw_sdk::image::PngWriter::new(
                self.shared.storage.as_ref(),
                path,
                u32::from(bounds.width),
                u32::from(bounds.height),
            )
            .ok()
        });
        let mut gray = Vec::new();
        let mut top = region.y - region.y % rows;
        while u32::from(top) < region.bottom() {
            self.framebuffer.set_top(top);
            if let Some(part) = region.intersection(&self.framebuffer.band()) {
                self.redraw(part);
                let view = self.framebuffer.view_rows(part);
                if let (Some(writer), Some(path)) = (writer.as_mut(), shot.as_deref()) {
                    for y in 0..view.height {
                        gray.clear();
                        gray.extend((0..view.width).map(|x| {
                            let c = view.pixel(x, y);
                            ((u32::from(c.r) * 77 + u32::from(c.g) * 150 + u32::from(c.b) * 29)
                                >> 8) as u8
                        }));
                        if writer
                            .row(self.shared.storage.as_ref(), path, &gray)
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                out(part.y, view);
            }
            top = top.saturating_add(rows);
            if top == u16::MAX {
                break;
            }
        }
        // Large pictures on the screen now: the next change of most of it
        // refreshes in full.
        let pictured = self.framebuffer.take_pictured();
        let screen = u32::from(bounds.width) * u32::from(bounds.height);
        let big = pictured.saturating_mul(100) >= screen.saturating_mul(PICTURES_PERCENT);
        let most = u32::from(region.width) * u32::from(region.height) * 2 >= screen;
        self.frame.pictures_shown = big || (self.frame.pictures_shown && !most);
        if let Some(path) = shot {
            let key = if writer.is_some_and(|w| w.is_complete()) {
                "core.screenshot-saved"
            } else {
                "core.screenshot-failed"
            };
            let text = self
                .shared
                .translator
                .format(key, &[("path", paperpaw_sdk::Arg::Str(&path))]);
            self.output.commands.push(Command::Notify(text));
        }
        // Read once for all the bands; not kept past the frame.
        self.shared.recall.clear();
    }

    /// Draws again what [`Kernel::render`] drew last, passing it the same
    /// way: for panel controllers that compare a new frame with the one
    /// shown (the X4's), whose memory for the shown one is written after
    /// a refresh, so the shell keeps no frame of its own. Call it right
    /// after the refresh, before anything else changes the screen; with
    /// changes not rendered yet it does nothing.
    pub fn rerender(&mut self, out: &mut dyn FnMut(u16, FrameView<'_>)) {
        if self.frame.unrendered.is_none() {
            self.frame.unrendered = self.frame.rendered;
            self.render(out);
        }
    }

    /// Draws the whole screen anew and passes it to `out` as
    /// [`Kernel::render`] does, while nothing on it changes: for a shell
    /// that sends a screenshot elsewhere, e.g. over USB. While changes wait
    /// to be rendered it does nothing and returns `false`.
    pub fn capture(&mut self, out: &mut dyn FnMut(u16, FrameView<'_>)) -> bool {
        if self.frame.unrendered.is_some() {
            return false;
        }
        let rendered = self.frame.rendered;
        self.frame.unrendered = Some(self.framebuffer.bounds());
        self.render(out);
        // What the panel was last sent stays what rerender sends again.
        self.frame.rendered = rendered;
        true
    }

    pub(crate) fn invalidate(&mut self, region: Rect) {
        self.frame.pending = Some(self.frame.pending.map_or(region, |p| p.union(&region)));
    }

    /// Redraws `region`: the topmost full screen and every overlay above
    /// it, each clipped to the region.
    pub(crate) fn redraw(&mut self, region: Rect) {
        let env = self.shared.env(self.now);
        let regions: Vec<Option<Rect>> = self.stack.iter().map(|s| s.screen.region(&env)).collect();
        // Overlays that wash out what lies below them.
        let dims: Vec<bool> = self
            .stack
            .iter()
            .zip(&regions)
            .map(|(s, own)| own.is_some() && s.screen.dims_below())
            .collect();
        let base = regions.iter().rposition(Option::is_none).unwrap_or(0);
        let bounds = self.content();
        let row = self.hint_row();
        let mut canvas = self.framebuffer.canvas();
        let mut canvas = canvas.clipped(region);
        canvas.clear(Color::WHITE);
        for (index, (active, own)) in self.stack[base..]
            .iter_mut()
            .zip(&regions[base..])
            .enumerate()
        {
            let Some(mut area) = own.unwrap_or(bounds).intersection(&region) else {
                continue;
            };
            // Less what the overlays above it cover: overlays paint all
            // of their region, so what lies below would not show (e.g.
            // the home screen under a menu whose focus moved: only the
            // rows above the menu, as frames are drawn in whole rows).
            let mut above = regions[base + index + 1..].iter().flatten();
            let Some(shown) = above.try_fold(area, |area, cover| uncovered(area, *cover)) else {
                continue;
            };
            area = shown;
            let _running = self.shared.meter.enter(active.id.namespace());
            active.screen.draw(&mut canvas.clipped(area), &env);
            if dims[base + index + 1..].iter().any(|d| *d) {
                canvas.wash(area);
            }
        }
        if let Some(row) = row
            && let Some(area) = row.intersection(&region)
        {
            let pressed = self.frame.mark.and_then(|(_, cell)| cell);
            paperpaw_sdk::ui::draw_button_hints(
                &mut canvas.clipped(area),
                &env,
                row,
                &self.hint.shown,
                &self.hint.held,
                pressed,
            );
        }
        // The busy mark in a corner, where no hint shows it.
        if let Some((mark, None)) = self.frame.mark
            && mark.intersection(&region).is_some()
        {
            let style = paperpaw_sdk::ui::UiStyle::new(&env);
            let mut canvas = canvas.clipped(mark);
            paperpaw_sdk::ui::fill_round(&mut canvas, &style, mark, Color::BLACK);
            let size = f32::from(mark.height) * 0.6;
            let x = f32::from(mark.x)
                + (f32::from(mark.width) - paperpaw_sdk::ui::icon_width(&env, BUSY_ICON, size))
                    / 2.0;
            paperpaw_sdk::ui::draw_icon(&mut canvas, &env, BUSY_ICON, size, x, mark, Color::WHITE);
        }
        // A message over everything.
        if let Some((notice, _)) = &mut self.notice
            && let Some(area) = notice.region(&env).unwrap_or(bounds).intersection(&region)
        {
            notice.draw(&mut canvas.clipped(area), &env);
        }
    }

    /// What the screens shown are, part by part (rows, buttons, headings
    /// with their text), as their drawing tells (see
    /// [`paperpaw_sdk::semantics`]): for reading the screen aloud,
    /// automation and tests. Draws them once, aside.
    pub fn describe(&mut self) -> Vec<paperpaw_sdk::semantics::Part> {
        paperpaw_sdk::semantics::parts(self.described())
    }

    /// What [`describe`](Self::describe) puts together: everything the
    /// drawing helpers told as they drew, in drawing order, each with the
    /// clip it was drawn in, e.g. for checks of text cut at a cell's edge.
    pub fn described(&mut self) -> Vec<paperpaw_sdk::semantics::Described> {
        let bounds = self.framebuffer.bounds();
        let Ok(mut aside) = Framebuffer::new(
            bounds.width,
            bounds.height,
            paperpaw_platform::PixelFormat::Mono1,
        ) else {
            return Vec::new();
        };
        let env = self.shared.env(self.now);
        let regions: Vec<Option<Rect>> = self.stack.iter().map(|s| s.screen.region(&env)).collect();
        let base = regions.iter().rposition(Option::is_none).unwrap_or(0);
        let mut described = Vec::new();
        {
            let mut canvas = aside.canvas().describing(&mut described);
            let content = Rect::new(0, 0, env.device.display.width, env.device.display.height);
            for (active, own) in self.stack[base..].iter_mut().zip(&regions[base..]) {
                let area = own.unwrap_or(content);
                active.screen.draw(&mut canvas.clipped(area), &env);
            }
            if content.height < bounds.height {
                let row = Rect::new(
                    0,
                    content.height,
                    bounds.width,
                    bounds.height - content.height,
                );
                paperpaw_sdk::ui::draw_button_hints(
                    &mut canvas.clipped(row),
                    &env,
                    row,
                    &self.hint.shown,
                    &self.hint.held,
                    None,
                );
            }
        }
        described
    }

    /// What the screen shown tells as it draws (see
    /// [`described`](Self::described)), alone: without the screens below
    /// an overlay and the button hints, e.g. for checks that move the
    /// focus through its parts. Draws it once, aside.
    pub fn described_active(&mut self) -> Vec<paperpaw_sdk::semantics::Described> {
        let bounds = self.framebuffer.bounds();
        let Ok(mut aside) = Framebuffer::new(
            bounds.width,
            bounds.height,
            paperpaw_platform::PixelFormat::Mono1,
        ) else {
            return Vec::new();
        };
        let env = self.shared.env(self.now);
        let Some(active) = self.stack.last_mut() else {
            return Vec::new();
        };
        let content = Rect::new(0, 0, env.device.display.width, env.device.display.height);
        let area = active.screen.region(&env).unwrap_or(content);
        let mut described = Vec::new();
        active.screen.draw(
            &mut aside.canvas().describing(&mut described).clipped(area),
            &env,
        );
        described
    }

    /// Times what the input started, and shows the busy mark first if it
    /// was slow the last time (or is a screen not opened yet), holding the
    /// work back until [`Kernel::proceed`].
    pub(crate) fn mark_if_slow(&mut self) -> Option<Effects> {
        let key = self
            .output
            .commands
            .iter()
            .find_map(|command| self.work_key(command))?;
        let started = self.shared.meter.micros()?;
        let slow = match self.timing.durations.iter().find(|(k, _)| *k == key) {
            Some((_, ms)) => *ms >= SLOW_MS,
            // Not timed yet: another screen may take long; an action is
            // taken to be quick until it was not.
            None => !key.starts_with("run:"),
        };
        self.timing.running = Some((key, started));
        if !self.busy_mark || !slow {
            return None;
        }
        let mark = self.mark_area();
        self.invalidate(mark.0);
        self.frame.mark = Some(mark);
        self.withheld = mem::take(&mut self.output.commands);
        self.output.request_refresh(RefreshMode::Fast);
        let mut effects = self.finish();
        effects.proceed = true;
        Some(effects)
    }

    /// Where the busy mark goes: the pressed button's hint, or a corner.
    pub(crate) fn mark_area(&self) -> (Rect, Option<usize>) {
        if let (Some(row), Some(cell)) = (self.hint_row(), self.hint.pressed) {
            let count = self.hint.buttons.len().max(1) as u32;
            let width = u32::from(row.width);
            let left = u32::from(row.x) + width * cell as u32 / count;
            let right = u32::from(row.x) + width * (cell as u32 + 1) / count;
            let area = Rect::new(left as u16, row.y, (right - left) as u16, row.height);
            return (area, Some(cell));
        }
        let metrics = paperpaw_sdk::Metrics::new(self.shared.device.display.dpi);
        let bounds = self.framebuffer.bounds();
        let size = (metrics.mm(9) as u16).min(bounds.width / 4);
        (Rect::new(bounds.width - size, 0, size, size), None)
    }

    /// Draws the whole screen again, refreshed fully: after the shell
    /// showed something of its own on the panel (e.g. a test picture).
    pub fn draw_again(&mut self, now: Instant) -> Effects {
        self.now = now;
        self.apply(Command::Redraw(RefreshMode::Full));
        self.finish()
    }

    /// The screen has another size, e.g. turned (see
    /// [`Kernel::rotation`]): every open screen is made anew for it, with
    /// what it was opened with, and the whole screen is drawn again.
    pub fn resize(&mut self, now: Instant, width: u16, height: u16) -> Effects {
        self.now = now;
        let display = self.shared.device.display;
        if (display.width, display.height) != (width, height) && width > 0 && height > 0 {
            // Bands take as much memory as before: fewer rows where the
            // screen got wider.
            let rows = match self.frame.band {
                Some(pixels) => {
                    let align = self.frame.row_align;
                    let rows = (pixels / u32::from(width)).min(u32::from(height)) as u16;
                    (rows / align * align).max(align).min(height)
                }
                None => height,
            };
            let (inverted, saturation, kept, coverage) = (
                self.framebuffer.is_inverted(),
                self.framebuffer.saturation(),
                self.framebuffer.pictures_kept(),
                self.framebuffer.mono_coverage(),
            );
            let before = (display.width, display.height, self.framebuffer.rows());
            // The old frame goes first, the new one taking its place: beside
            // it, it would split the largest free block where memory is
            // short.
            if let Ok(none) = Framebuffer::new(1, 1, display.format) {
                self.framebuffer = none;
            }
            let made = Framebuffer::banded(width, height, rows, display.format);
            let turned = made.is_ok();
            if let Ok(mut framebuffer) =
                made.or_else(|_| Framebuffer::banded(before.0, before.1, before.2, display.format))
            {
                framebuffer.set_inverted(inverted);
                framebuffer.set_pictures_kept(kept);
                framebuffer.set_saturation(saturation);
                framebuffer.set_mono_coverage(coverage);
                self.framebuffer = framebuffer;
            }
            if turned {
                self.shared.device.display.width = width;
                self.shared.device.display.height = height;
                self.hint.height = self.wanted_hint_height();
                self.remake_screens();
            }
        }
        self.finish()
    }
}
