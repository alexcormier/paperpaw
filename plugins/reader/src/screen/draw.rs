//! Drawing the page: its text and pictures, highlights, links, the
//! plugins' layers, the bookmark ribbon, the way back from a link and the
//! status lines.

use super::*;

use paperpaw_sdk::layers::{LineBox, PageView};

impl Book {
    /// The lines of the page as layers see them: each as wide as its
    /// column, as high as a line of body text, on its baseline.
    pub(super) fn line_boxes(&self) -> Vec<LineBox> {
        let t = &self.typography;
        let text = Rect::new(t.x as u16, t.y as u16, t.width as u16, t.height as u16);
        self.page
            .lines
            .iter()
            .filter_map(|line| {
                let (first, last) = (line.words.first()?.0, line.words.last()?.1);
                let x = (t.x + line.x) as u32;
                let column = t
                    .columns
                    .iter()
                    .find(|c| u32::from(c.x) <= x && x < c.right())
                    .copied()
                    .unwrap_or(text);
                let top = (t.y + line.baseline - t.ascent).max(0.0);
                Some(LineBox::new(
                    Rect::new(column.x, top as u16, column.width, t.line_height as u16),
                    Locator::new(line.section, first),
                    Locator::new(line.section, last),
                ))
            })
            .collect()
    }

    /// The page as layers see it, with its `lines`.
    pub(super) fn page_view<'a>(&'a self, lines: &'a [LineBox]) -> PageView<'a> {
        let t = &self.typography;
        let text = Rect::new(t.x as u16, t.y as u16, t.width as u16, t.height as u16);
        PageView::new(&self.path, self.page.start, self.page.end, text)
            .language(&self.language)
            .columns(&t.columns)
            .lines(lines)
            .status(t.top_status, t.status)
    }

    /// The right edge of the text and the layers' rooms beside it.
    fn right_edge(&self) -> f32 {
        let t = &self.typography;
        t.rooms
            .iter()
            .map(|(_, room)| room.right() as f32)
            .fold(t.x + t.width, f32::max)
    }

    /// Tells the plugin of layer `layer` its room was tapped, at `at`
    /// (its middle when chosen with the buttons).
    pub(super) fn tap_layer(&self, layer: &Id, at: Option<Point>, cx: &mut Cx<'_, '_>) {
        let Some((_, room)) = self.typography.rooms.iter().find(|(id, _)| id == layer) else {
            return;
        };
        let at = at.unwrap_or_else(|| {
            Point::new(
                i32::from(room.x) + i32::from(room.width) / 2,
                i32::from(room.y) + i32::from(room.height) / 2,
            )
        });
        let lines = self.line_boxes();
        let line = self.page_view(&lines).line_at(at.x, at.y).map(|l| l.start);
        let mut tapped = events::PageLayerTapped::new(layer.clone(), self.path.clone(), at.x, at.y);
        if let Some(line) = line {
            tapped = tapped.section(line.section).offset(line.offset);
        }
        cx.command(events::PAGE_LAYER_TAPPED.emit(tapped));
    }

    /// The page's pictures and text, each word in its ink (`inks`, by the
    /// words' order as [`Book::words`] has them; black where none).
    pub(super) fn draw_page(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, inks: &[Option<Color>]) {
        let t = &self.typography;
        let Some(faces) = Faces::new(env, t, self.embedded.as_ref()) else {
            return;
        };
        for image in &self.page.images {
            paperpaw_sdk::reading::draw_image(canvas, env, t, image);
        }
        paperpaw_sdk::reading::draw_lines(canvas, &faces, t, (t.x, t.y), &self.page.lines, inks);
    }

    /// Where *Back* is offered on the page a link led to: a small button
    /// in the top margin, on the left.
    /// On web pages it is always there: back to the page before.
    pub(super) fn back_area(&self, env: &Env<'_>) -> Option<Rect> {
        let landed = !self.returns.is_empty() && self.landed == Some(self.page.start);
        if !landed && !self.web() {
            return None;
        }
        let style = UiStyle::new(env);
        let font = style.font(env)?;
        let label = env.i18n.tr("reader.link-back");
        let pad = style.gap as f32;
        let width = font.measure(style.small, label) + 2.0 * pad;
        // Within the top margin where it is wide enough, so it covers no
        // text.
        let line = font.line_metrics(style.small).line_height;
        let height = (line + pad).min(self.typography.y - 4.0).max(line);
        let y = ((self.typography.y - height) / 2.0).max(0.0);
        Some(Rect::new(
            self.typography.x as u16,
            y as u16,
            width as u16,
            height as u16,
        ))
    }

    pub(super) fn draw_back(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let Some(area) = self.back_area(env) else {
            return;
        };
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return;
        };
        let (x, y, w, h) = (
            i32::from(area.x),
            i32::from(area.y),
            u32::from(area.width),
            u32::from(area.height),
        );
        canvas.fill_rect(x, y, w, h, Color::WHITE);
        canvas.stroke_rect(x, y, w, h, 1, Color::BLACK);
        let label = env.i18n.tr("reader.link-back");
        ui::draw_label(
            canvas,
            &font,
            style.small,
            f32::from(area.x) + style.gap as f32,
            area,
            label,
            Color::BLACK,
        );
    }

    /// Where a tap sets or removes a bookmark: the top right corner.
    pub(super) fn corner_area(&self, env: &Env<'_>) -> Rect {
        let display = env.device.display;
        let size = (env.metrics.mm(12) as u16).min(display.width / 4);
        Rect::new(display.width - size, 0, size, size)
    }

    pub(super) fn ribbon_area(&self, env: &Env<'_>) -> Rect {
        let t = &self.typography;
        let width = env.metrics.mm(3) as u16;
        let height = (t.y * 0.8) as u16;
        let x = self.right_edge() as u16 - width;
        Rect::new(x, 0, width, height.max(width))
    }

    /// A ribbon with a notch, like a bookmark in a book.
    pub(super) fn draw_ribbon(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let r = self.ribbon_area(env);
        let notch = u32::from(r.width) / 2;
        let body = u32::from(r.height).saturating_sub(notch);
        canvas.fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            body,
            Color::BLACK,
        );
        // Both sides of the notch narrow towards the bottom.
        for row in 0..notch {
            let side = notch - row;
            let y = i32::from(r.y) + (body + row) as i32;
            canvas.fill_rect(i32::from(r.x), y, side, 1, Color::BLACK);
            canvas.fill_rect(
                i32::from(r.x) + (u32::from(r.width) - side) as i32,
                y,
                side,
                1,
                Color::BLACK,
            );
        }
    }

    /// A note's sign in the right margin, beside the first line of its
    /// highlight (the highlights themselves are marks, see `marks`).
    pub(super) fn draw_note_signs(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        words: &[WordBox],
    ) {
        let highlights: Vec<&Mark> = self
            .marks
            .iter()
            .filter(|m| m.kind == Kind::Highlight && m.overlaps(self.page.start, self.page.end))
            .collect();
        if highlights.is_empty() {
            return;
        }
        let style = UiStyle::new(env);
        let size = style.small;
        let columns = &self.typography.columns;
        for mark in highlights.iter().filter(|m| m.noted) {
            if let Some(first) = words.iter().find(|w| mark.overlaps(w.start, w.end)) {
                // Beside its column; beside the layers' rooms past the last.
                let x = u32::from(first.area.x);
                let right = match columns.iter().position(|c| x < c.right()) {
                    Some(i) if i + 1 < columns.len() => columns[i].right() as f32,
                    _ => self.right_edge(),
                };
                let x = right + (style.gap as f32) / 2.0;
                ui::draw_icon(canvas, env, "note", size, x, first.area, Color::BLACK);
            }
        }
    }

    /// What status items get to know about the book.
    pub(super) fn status_context(&self, env: &Env<'_>) -> Params {
        let reading = self.reading(env);
        let in_chapter = match (reading.page, reading.chapter_left) {
            (Some(page), Some(left)) => self
                .chapter_first(env)
                .map(|first| (page.saturating_sub(first), left)),
            _ => None,
        };
        let mut context = reading
            .to_params()
            .with(status_context::PAGE, self.position_label(env));
        if let Some((page, left)) = in_chapter {
            context.set(status::CHAPTER_PAGE_CONTEXT, format!("{page}"));
            context.set(status::CHAPTER_PAGES_CONTEXT, format!("{}", page + left));
        }
        if let Some((_, _, chapter)) = &self.chapter
            && !chapter.is_empty()
        {
            context.set(status_context::CHAPTER, chapter.clone());
        }
        if !self.chapter_marks.is_empty() {
            context.set(status_context::CHAPTERS, self.chapter_marks.clone());
        }
        if let Some(icon) = self.hands_free {
            context.set(status::ALOUD_CONTEXT, icon);
        }
        if let Some(page) = self.document.page_label(env.storage, self.page.start) {
            context.set(status::PRINTED_CONTEXT, page);
        }
        context
    }

    /// Where the chapters begin, as thousandths of the book (see
    /// [`chapter_ticks`]).
    pub(super) fn chapter_marks(&self, env: &Env<'_>) -> String {
        let (document, storage) = (&self.document, env.storage);
        let entries: Vec<(u8, u32)> = (0..document.toc_len(storage).min(500))
            .filter_map(|index| document.toc_entry(storage, index))
            .map(|entry| (entry.level, (document.progress(entry.at) * 1000.0) as u32))
            .collect();
        let marks: Vec<String> = chapter_ticks(&entries)
            .iter()
            .map(ToString::to_string)
            .collect();
        marks.join(",")
    }

    /// The status line at the top (or bottom) that `point` is on: its
    /// place reaches to the edge of the screen.
    pub(super) fn status_hit(&self, env: &Env<'_>, point: Point) -> Option<bool> {
        let height = env.device.display.height;
        self.status_areas().find_map(|(area, top)| {
            let zone = if top {
                Rect::new(0, 0, env.device.display.width, area.y + area.height)
            } else {
                Rect::new(0, area.y, env.device.display.width, height - area.y)
            };
            zone.contains(point).then_some(top)
        })
    }

    /// The status lines' areas.
    pub(super) fn status_areas(&self) -> impl Iterator<Item = (Rect, bool)> {
        let t = &self.typography;
        t.top_status
            .map(|r| (r, true))
            .into_iter()
            .chain(t.status.map(|r| (r, false)))
    }

    /// Draws the status lines in the canvas's band; in the others what
    /// they show is not even asked (the X4 draws a page in eight bands).
    pub(super) fn draw_status(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let clip = canvas.clip();
        let mut areas = self
            .status_areas()
            .filter(|(area, _)| area.intersection(&clip).is_some())
            .peekable();
        if areas.peek().is_none() {
            return;
        }
        let context = self.status_context(env);
        for (area, top) in areas {
            canvas.fill(area, Color::WHITE);
            let id = status::current(env, top, &self.status_lines[usize::from(!top)]);
            let frame = paperpaw_sdk::StatusFrame::new(
                area,
                top,
                status::size(env),
                status::color(env),
                &context,
            );
            env.status_items.draw_line(id, canvas, env, &frame);
        }
    }
}

/// Ticks on a bar across the book that deeper levels of the contents
/// may bring it to: more crowd it into a line.
const MAX_TICKS: usize = 60;

/// The places of the contents' `entries` (level, thousandths) that get a
/// tick: every top-level entry, and the deeper ones (chapters in parts)
/// down to the deepest level whose entries and those above still leave
/// the ticks apart.
pub(super) fn chapter_ticks(entries: &[(u8, u32)]) -> Vec<u32> {
    let deepest = entries.iter().map(|&(level, _)| level).max().unwrap_or(0);
    let shown = (0..=deepest)
        .rev()
        .find(|&depth| entries.iter().filter(|&&(level, _)| level <= depth).count() <= MAX_TICKS)
        .unwrap_or(0);
    let mut ticks: Vec<u32> = entries
        .iter()
        .filter(|&&(level, _)| level <= shown)
        .map(|&(_, at)| at)
        .collect();
    ticks.dedup();
    ticks
}
