//! Labels, icons, headings, messages and empty screens: text drawn in
//! its room, shortened where it does not fit.

#[allow(clippy::wildcard_imports)]
use super::*;

/// Lines of a row's note, e.g. a setting's description.
pub const NOTE_LINES: usize = 2;

/// Line height of notes, relative to the small text size.
pub(super) const NOTE_LINE_HEIGHT: f32 = 1.3;

/// Draws icon `name` ([`icons`](crate::icons)) `size` pixels high,
/// vertically centred in `area`, starting at `x`; returns its width, 0 if
/// there is no icon font or no such icon.
pub fn draw_icon(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    name: &str,
    size: f32,
    x: f32,
    area: Rect,
    color: Color,
) -> f32 {
    let Some((font, ch)) = icon(env, name) else {
        return 0.0;
    };
    let mut text = [0u8; 4];
    let text = ch.encode_utf8(&mut text);
    draw_text(canvas, &font, size, x, area, text, color)
}

/// The width of icon `name` at `size` pixels, 0 if it cannot be drawn.
pub fn icon_width(env: &Env<'_>, name: &str, size: f32) -> f32 {
    icon(env, name).map_or(0.0, |(font, ch)| font.advance(size, ch))
}

/// Where icon `name`'s ink begins and ends across at `size`, from where it
/// is drawn (see [`FontHandle::ink_across`]); `None` without a drawing.
pub fn icon_ink(env: &Env<'_>, name: &str, size: f32) -> Option<(i32, i32)> {
    let (font, ch) = icon(env, name)?;
    let mut text = [0u8; 4];
    font.ink_across(size, ch.encode_utf8(&mut text))
}

/// The icon font and the character of icon `name`, where it has a drawing
/// (its own, or an icon pack's).
pub(super) fn icon<'f>(env: &Env<'f>, name: &str) -> Option<(FontHandle<'f>, char)> {
    let font = env.fonts.bound(crate::builtin::fonts::ICONS.as_str())?;
    let ch = crate::icons::char(name)?;
    font.has_char(ch).then_some((font, ch))
}

/// The height of a section's heading in a list (see [`Rows`]): small
/// text with a little room above it.
pub fn heading_height(env: &Env<'_>, style: &UiStyle) -> u16 {
    let line = style.font(env).map_or(style.small * 1.3, |f| {
        f.line_metrics(style.small).line_height
    });
    (line * 1.25) as u16 + style.gap as u16
}

/// Draws a section's heading in `rect`: small and gray, low in its room,
/// so it reads as belonging to the entries below; between the margins, a
/// long one shortened with "…" as the rows' titles are.
pub fn draw_heading(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    text: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let line = font.line_metrics(style.small).line_height as u16;
    let area = Rect::new(
        rect.x,
        rect.y + rect.height.saturating_sub(line + style.gap as u16 / 2),
        rect.width,
        line,
    );
    canvas.describe(Role::Heading, text, area, false, None);
    // Ending at the right margin, where the rows' values end: what a last
    // letter reaches past its width stays in the row.
    let margin = style.margin as u16;
    let text_area = Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(margin),
        area.height,
    );
    let x = f32::from(rect.x) + style.margin as f32;
    draw_label(
        canvas,
        &font,
        style.small,
        x,
        text_area,
        text,
        style.faint(90),
    );
}

/// The switch of an on/off setting at the right end of `row`.
pub fn switch_rect(style: &UiStyle, row: Rect) -> Rect {
    let height = (button_height(row) * 3 / 4).max(8);
    let width = height * 2;
    Rect::new(
        (row.x + row.width).saturating_sub(width + style.margin as u16),
        row.y + (row.height - height) / 2,
        width,
        height,
    )
}

/// Draws a switch at the right end of `row`: filled with the knob on the
/// right when `on`, outlined with the knob on the left when off. `inverted`
/// for a focused (black) row.
pub fn draw_switch(canvas: &mut Canvas<'_>, style: &UiStyle, row: Rect, on: bool, inverted: bool) {
    let r = switch_rect(style, row);
    canvas.describe(Role::Switch, "", r, false, Some(on));
    let (ink, paper) = if inverted {
        (Color::WHITE, Color::BLACK)
    } else {
        (Color::BLACK, Color::WHITE)
    };
    let border = button_border(style);
    let radius = style.radius(u32::from(r.height));
    if on {
        canvas.fill_round(r, radius, ink);
    } else {
        canvas.fill_round(r, radius, paper);
        canvas.stroke_round(r, border, radius, ink);
    }
    let pad = (border as u16 * 2).max(3);
    let knob = r.height.saturating_sub(2 * pad);
    let x = if on {
        r.x + r.width - pad - knob
    } else {
        r.x + pad
    };
    let knob_rect = Rect::new(x, r.y + pad, knob, knob);
    let knob_radius = if style.pill {
        u32::from(knob) / 2
    } else {
        radius.saturating_sub(u32::from(pad))
    };
    canvas.fill_round(knob_rect, knob_radius, if on { paper } else { ink });
}

/// Icons in lists are this much larger than the text beside them.
pub const ICON_SCALE: f32 = 1.25;

/// The room icons take at the start of list rows (icon and gap), or 0
/// without an icon font or when `any` row has none.
pub fn icon_room(env: &Env<'_>, style: &UiStyle, any: bool) -> u16 {
    if any && has_icons(env) {
        (style.text * ICON_SCALE) as u16 + style.gap as u16
    } else {
        0
    }
}

/// Draws a list row's icon at the margin, centred on the row's first
/// line (`rect`), and returns the rest of the row for its label: the
/// label starts where it would after any icon, so rows line up.
pub fn draw_row_icon(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    room: u16,
    icon: Option<&str>,
    ink: Color,
) -> Rect {
    if let Some(icon) = icon.filter(|_| room > 0) {
        let size = style.text * ICON_SCALE;
        // Centred by its ink in a column as wide as the icons are high:
        // icons drawn with more room on one side line up with the others.
        let column = f32::from(rect.x) + style.margin as f32;
        let x = icon_ink(env, icon, size).map_or(column, |(left, right)| {
            column + (size - (right - left) as f32) / 2.0 - left as f32
        });
        draw_icon(canvas, env, icon, size, x, rect, ink);
    }
    Rect::new(
        rect.x + room,
        rect.y,
        rect.width.saturating_sub(room),
        rect.height,
    )
}

/// Draws a row with `title` and a switch for `on` at its end, with
/// `note` below when `noted` (see [`draw_noted_row`]).
#[allow(clippy::too_many_arguments)]
pub fn draw_switch_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    title: &str,
    note: &str,
    on: bool,
) {
    let band = if note.is_empty() {
        rect
    } else {
        title_area(style, rect)
    };
    let switch = switch_rect(style, band);
    // The title ends a gap before the switch.
    let room = (switch.x + style.margin as u16).saturating_sub(band.x + style.gap as u16);
    let title_rect = Rect::new(band.x, band.y, room, band.height);
    draw_value_row(canvas, env, style, title_rect, focused, title, "");
    if !note.is_empty() {
        draw_note(canvas, env, style, rect, focused, note);
    }
    draw_switch(canvas, style, band, on, focused && style.dark_focus());
}

/// Whether icons can be drawn (an icon font is there).
pub fn has_icons(env: &Env<'_>) -> bool {
    env.fonts
        .bound(crate::builtin::fonts::ICONS.as_str())
        .is_some()
}

/// The translation of `key` for how the device is used: where there is
/// no touch screen, `<key>.buttons` if there is one (e.g. "Select a rule
/// to pause it" for "Tap a rule to pause it"), else `key`'s own. Notes
/// that tell what a tap or a hold does use it, so they never speak of
/// taps on a device without a touch screen.
pub fn tr_input<'a>(env: &Env<'a>, key: &'a str) -> &'a str {
    if !env.device.input.touch
        && let Some(text) = env.i18n.get(&format!("{key}.buttons"))
    {
        return text;
    }
    env.i18n.tr(key)
}

/// Draws `text` vertically centred in `area`, starting at `x`; text
/// longer than the rest of the area is shortened with "…", so a label
/// never runs into what is next to it.
pub fn draw_label(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    x: f32,
    area: Rect,
    text: &str,
    color: Color,
) -> f32 {
    let room = f32::from(area.x) + f32::from(area.width) - x;
    let at = Rect::new(x.max(0.0) as u16, area.y, room.max(0.0) as u16, area.height);
    canvas.describe(Role::Text, text, at, false, None);
    draw_text(canvas, font, size, x, area, text, color)
}

/// [`draw_label`] without telling what it draws (icons say nothing).
pub(super) fn draw_text(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    x: f32,
    area: Rect,
    text: &str,
    color: Color,
) -> f32 {
    let metrics = font.line_metrics(size);
    let baseline = i32::from(area.y)
        + ((f32::from(area.height) + metrics.ascent - metrics.descent) / 2.0) as i32;
    let room = f32::from(area.x) + f32::from(area.width) - x;
    // A pixel of slack for rounding: callers often measured to fit.
    if room > 0.0 && font.measure(size, text) > room + 1.0 {
        let text = fit(font, size, text, room);
        return font.draw(canvas, size, x, baseline, &text, color);
    }
    font.draw(canvas, size, x, baseline, text, color)
}

/// The baseline that centres the ink of `text` at `size` in `band`,
/// rather than its line, which leaves more room above the letters than
/// below them: what headers use, so their words sit in the middle as
/// their signs do.
pub(super) fn ink_baseline(font: &FontHandle<'_>, band: Rect, size: f32, text: &str) -> i32 {
    let (top, bottom) = font.ink(size, text).unwrap_or_else(|| {
        // No ink at all: as a line would be.
        let metrics = font.line_metrics(size);
        (-(metrics.ascent as i32), -(metrics.descent as i32))
    });
    let room = i32::from(band.height) - (bottom - top);
    // Rounded down: an odd pixel goes below, where the eye expects it.
    i32::from(band.y) + room.div_euclid(2) - top
}

/// [`draw_label`] on `baseline` rather than centred in `area`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_label_on(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    x: f32,
    area: Rect,
    baseline: i32,
    text: &str,
    color: Color,
) -> f32 {
    let room = f32::from(area.x) + f32::from(area.width) - x;
    let at = Rect::new(x.max(0.0) as u16, area.y, room.max(0.0) as u16, area.height);
    canvas.describe(Role::Text, text, at, false, None);
    if room > 0.0 && font.measure(size, text) > room + 1.0 {
        let text = fit(font, size, text, room);
        return font.draw(canvas, size, x, baseline, &text, color);
    }
    font.draw(canvas, size, x, baseline, text, color)
}

/// A title and a value side by side in `width` pixels: both as they are
/// if they fit, else shortened with "…". The title says what the row is,
/// so it keeps up to 60 % of the width; the value gets the rest. Returns
/// the texts and the value's width.
pub fn fit_pair(
    font: &FontHandle<'_>,
    size: f32,
    title: &str,
    value: &str,
    width: f32,
    gap: f32,
) -> (String, String, f32) {
    let room = (width - gap).max(0.0);
    let title_width = font.measure(size, title).min(room * 0.6);
    let value_room = room - title_width;
    // A pixel of slack for rounding, as in [`draw_label`].
    let value = fit(font, size, value, value_room + 1.0);
    let value_width = font.measure(size, &value);
    let title = fit(font, size, title, (room - value_width).max(0.0) + 1.0);
    (title, value, value_width)
}

/// Draws wrapped, centred message text in `area`.
pub fn draw_message(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    text: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    canvas.describe(Role::Text, text, area, false, None);
    let width = f32::from(area.width) - 2.0 * style.margin as f32;
    let line_height = font.line_metrics(style.text).line_height;
    let mut lines = font.wrap(style.text, text, width);
    // As many lines as the area holds; the last one says there is more.
    let fits = ((f32::from(area.height) / line_height) as usize).max(1);
    let cut = lines.len() > fits;
    lines.truncate(fits);
    let total = line_height * lines.len() as f32;
    let mut baseline =
        f32::from(area.y) + (f32::from(area.height) - total).max(0.0) / 2.0 + line_height * 0.8;
    let count = lines.len();
    for (n, range) in lines.into_iter().enumerate() {
        let line = if cut && n + 1 == count {
            let rest = text[range.start..].trim_end();
            Cow::Owned(fit(&font, style.text, &format!("{rest} …"), width))
        } else {
            Cow::Borrowed(&text[range])
        };
        let x = f32::from(area.x) + (f32::from(area.width) - font.measure(style.text, &line)) / 2.0;
        font.draw(canvas, style.text, x, baseline as i32, &line, Color::BLACK);
        baseline += line_height;
    }
}

/// A note at the bottom of a screen (what a tap or a hold does there, or
/// what just happened): a light line above it, then `text` in small gray
/// letters, wrapped to two lines (the last one shortened if need be).
pub fn draw_note_line(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    text: &str,
) {
    let margin = style.margin as u16;
    let width = area.width.saturating_sub(2 * margin);
    if style.separators != Separators::None {
        canvas.fill_tint(
            Rect::new(area.x + margin, area.y, width, style.line as u16),
            Color::gray(190),
        );
    }
    let Some(font) = style.font(env) else {
        return;
    };
    let top = area.y + style.line as u16;
    let height = area.height.saturating_sub(style.line as u16);
    let line = font.line_metrics(style.small).line_height;
    let room = f32::from(width);
    let mut lines = font.wrap(style.small, text, room);
    let fits = ((f32::from(height) / line) as usize).clamp(1, 2);
    let cut = lines.len() > fits;
    lines.truncate(fits);
    let count = lines.len();
    let mut y = f32::from(top) + (f32::from(height) - line * count as f32).max(0.0) / 2.0;
    for (n, range) in lines.into_iter().enumerate() {
        let shown = if cut && n + 1 == count {
            fit(
                &font,
                style.small,
                &format!("{} …", text[range.start..].trim_end()),
                room,
            )
        } else {
            text[range].to_string()
        };
        let row = Rect::new(area.x + margin, y as u16, width, line as u16);
        draw_label(
            canvas,
            &font,
            style.small,
            f32::from(row.x),
            row,
            &shown,
            style.faint(60),
        );
        y += line;
    }
}

/// `text` shortened with an ellipsis to fit `width` pixels.
pub fn fit(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> String {
    if font.measure(size, text) <= width {
        return text.to_string();
    }
    let ellipsis = font.measure(size, "…");
    let mut result = String::new();
    let mut used = 0.0;
    for c in text.chars() {
        let w = font.advance(size, c);
        if used + w + ellipsis > width {
            break;
        }
        used += w;
        result.push(c);
    }
    result.push('…');
    result
}

/// An empty screen or list, saying what would be here and how it gets
/// filled: a large `icon` in gray, a short `title`, and `text` below it,
/// together a little above the middle of `area`. The icon is left out
/// where there is not room for all of it, the text shortened last.
#[allow(clippy::too_many_arguments)]
pub fn draw_empty(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    icon: &str,
    title: &str,
    text: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let margin = style.margin as f32 * 2.0;
    let width = (f32::from(area.width) - 2.0 * margin).max(1.0);
    let icon_size = style.title * 2.4;
    let gap = style.gap as f32;
    let title_line = font.line_metrics(style.title).line_height;
    let line = font.line_metrics(style.text).line_height;
    let mut lines = if text.is_empty() {
        Vec::new()
    } else {
        font.wrap(style.text, text, width)
    };
    let room = f32::from(area.height);
    let text_height = |n: usize| line * n as f32;
    let words = if title.is_empty() { 0.0 } else { title_line };
    let icon_w = icon_width(env, icon, icon_size);
    let with_icon = icon_w > 0.0
        && icon_w <= f32::from(area.width)
        && icon_size + gap * 2.0 + words + gap + text_height(lines.len()) <= room;
    let icon_room = if with_icon {
        icon_size + gap * 2.0
    } else {
        0.0
    };
    // As many lines of the text as fit, the last saying there is more.
    let fits = ((room - icon_room - words - gap) / line).max(0.0) as usize;
    let cut = lines.len() > fits;
    lines.truncate(fits);
    let total =
        icon_room + words + if lines.is_empty() { 0.0 } else { gap } + text_height(lines.len());
    // A little above the middle, where the eye looks first.
    let mut y = f32::from(area.y) + ((room - total) * 0.42).max(0.0);
    let centre = |w: f32| f32::from(area.x) + (f32::from(area.width) - w) / 2.0;
    if with_icon {
        let w = icon_w;
        let at = Rect::new(area.x, y as u16, area.width, icon_size as u16);
        draw_icon(
            canvas,
            env,
            icon,
            icon_size,
            centre(w),
            at,
            style.faint(120),
        );
        // A small paw beside the icon, as if it had just walked past;
        // the icon stays in the middle.
        let size = icon_size * 0.4;
        let below = Rect::new(
            area.x,
            (y + icon_size - size) as u16,
            area.width,
            size as u16,
        );
        let x = centre(w) + w + gap / 2.0;
        // Only where it fits beside the icon (not in a narrow area).
        let right = f32::from(area.x) + f32::from(area.width);
        if x + icon_width(env, PAW_ICON, size) <= right {
            draw_icon(canvas, env, PAW_ICON, size, x, below, style.faint(120));
        }
        y += icon_room;
    }
    if !title.is_empty() {
        let shown = fit(&font, style.title, title, width);
        let at = Rect::new(area.x, y as u16, area.width, title_line as u16);
        draw_label(
            canvas,
            &font,
            style.title,
            centre(font.measure(style.title, &shown)),
            at,
            &shown,
            Color::BLACK,
        );
        y += title_line + gap;
    }
    if lines.is_empty() {
        return;
    }
    let block = Rect::new(
        area.x,
        y as u16,
        area.width,
        text_height(lines.len()) as u16,
    );
    canvas.describe(Role::Text, text, block, false, None);
    let ascent = font.line_metrics(style.text).ascent;
    let count = lines.len();
    for (n, range) in lines.into_iter().enumerate() {
        let shown = if cut && n + 1 == count {
            fit(
                &font,
                style.text,
                &format!("{} …", text[range.start..].trim_end()),
                width,
            )
        } else {
            text[range].trim().to_string()
        };
        let x = centre(font.measure(style.text, &shown));
        font.draw(
            canvas,
            style.text,
            x,
            (y + ascent) as i32,
            &shown,
            style.faint(70),
        );
        y += line;
    }
}
