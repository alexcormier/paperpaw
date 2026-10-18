//! Drawing the page: its text and pictures, highlights, links, the
//! bookmark ribbon, the way back from a link and the status lines.

use super::*;

impl Book {
    pub(super) fn draw_page(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let t = &self.typography;
        let Some(faces) = Faces::new(env, t, self.embedded.as_ref()) else {
            return;
        };
        let clip = canvas.clip();
        for image in &self.page.images {
            draw_image(canvas, env, t, image);
        }
        for line in &self.page.lines {
            let baseline = (t.y + line.baseline + 0.5) as i32;
            // Lines outside the part being drawn (e.g. another band).
            let top = baseline - (2.0 * t.line_height) as i32;
            let bottom = baseline + t.line_height as i32;
            if bottom < i32::from(clip.y) || top > clip.bottom() as i32 {
                continue;
            }
            let mut x = t.x + line.x;
            for run in &line.runs {
                let text = &line.text[run.start..run.end];
                let space = faces.space(run.face) + line.word_spacing;
                for (i, word) in text.split(' ').enumerate() {
                    if i > 0 {
                        x += space;
                    }
                    x += faces.draw(canvas, run.face, x, baseline, word);
                }
            }
        }
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

    /// Links underlined thinly, so they can be found without standing out.
    /// Links to the web only if a browser is there to open them.
    pub(super) fn draw_links(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, words: &[WordBox]) {
        let web = env.has_slot(&slots::BROWSER);
        let followed = |i: usize| {
            self.link_of(&words[i])
                .is_some_and(|link| web || !quireink_sdk::document::is_external(&link.href))
        };
        for b in selection::line_boxes(words, followed) {
            canvas.fill_rect(
                i32::from(b.x),
                b.bottom() as i32 - 2,
                u32::from(b.width),
                1,
                Color::gray(90),
            );
        }
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
        let x = (t.x + t.width) as u16 - width;
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

    /// Highlights behind the text: light gray, one shape per line.
    pub(super) fn draw_highlights(
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
        let boxes = selection::line_boxes(words, |i| {
            highlights
                .iter()
                .any(|m| m.overlaps(words[i].start, words[i].end))
        });
        let underline = env.settings.read(&crate::settings::HIGHLIGHT_STYLE)
            == crate::settings::HIGHLIGHT_UNDERLINE;
        let line = env.metrics.tenth_mm(4).max(2);
        for b in boxes {
            if underline {
                canvas.fill_rect(
                    i32::from(b.x),
                    b.bottom() as i32 - line as i32,
                    u32::from(b.width),
                    line,
                    Color::BLACK,
                );
            } else {
                canvas.fill_rect(
                    i32::from(b.x) - 2,
                    i32::from(b.y),
                    u32::from(b.width) + 4,
                    u32::from(b.height),
                    Color::gray(205),
                );
            }
        }
        // A note's sign in the right margin, beside its first line.
        let style = UiStyle::new(env);
        let size = style.small;
        let x = self.typography.x + self.typography.width + (style.gap as f32) / 2.0;
        for mark in highlights.iter().filter(|m| m.noted) {
            if let Some(first) = words.iter().find(|w| mark.overlaps(w.start, w.end)) {
                ui::draw_icon(canvas, env, "note", size, x, first.area, Color::BLACK);
            }
        }
    }

    /// What status items get to know about the book.
    pub(super) fn status_context(&self, env: &Env<'_>) -> Params {
        let mut context = self
            .reading(env)
            .to_params()
            .with(status_context::PAGE, self.position_label(env));
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

    /// Where the top-level chapters begin, as thousandths of the book.
    pub(super) fn chapter_marks(&self, env: &Env<'_>) -> String {
        let (document, storage) = (&self.document, env.storage);
        let mut marks: Vec<u32> = Vec::new();
        for index in 0..document.toc_len(storage).min(500) {
            if let Some(entry) = document.toc_entry(storage, index).filter(|e| e.level == 0) {
                marks.push((document.progress(entry.at) * 1000.0) as u32);
            }
        }
        marks.dedup();
        let marks: Vec<String> = marks.iter().map(ToString::to_string).collect();
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

    pub(super) fn draw_status(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let context = self.status_context(env);
        for (area, top) in self.status_areas() {
            canvas.fill_rect(
                i32::from(area.x),
                i32::from(area.y),
                u32::from(area.width),
                u32::from(area.height),
                Color::WHITE,
            );
            let id = status::current(env, top, &self.status_lines[usize::from(!top)]);
            let frame = quireink_sdk::StatusFrame {
                area,
                top,
                size: status::size(env),
                color: status::color(env),
                context: &context,
            };
            env.status_items.draw_line(id, canvas, env, &frame);
        }
    }
}

/// Draws an image at its place on the page.
pub(super) fn draw_image(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    t: &Typography,
    image: &PageImage,
) {
    let left = (t.x + image.x + 0.5) as i32;
    let top = (t.y + image.y + 0.5) as i32;
    let (w, h) = ((image.width + 0.5) as i32, (image.height + 0.5) as i32);
    quireink_sdk::ui::draw_image(
        canvas,
        env.storage,
        &image.image,
        Rect::new(left as u16, top as u16, w as u16, h as u16),
    );
}
