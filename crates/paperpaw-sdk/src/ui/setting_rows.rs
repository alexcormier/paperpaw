//! Settings drawn as rows: values, notes, numbers and steppers, and
//! stepping a setting's value.

#[allow(clippy::wildcard_imports)]
use super::*;

// ---- Settings as rows --------------------------------------------------

/// The part of a noted row (see [`UiStyle::noted_row`]) that holds the
/// title and the value, or a stepper; the note goes below it.
pub fn title_area(style: &UiStyle, rect: Rect) -> Rect {
    let band = (style.title_band() as u16).min(rect.height);
    Rect::new(rect.x, rect.y + style.gap as u16 / 2, rect.width, band)
}

/// Draws a row with `title` and `value` on its first line and `note` in
/// small text below, wrapped to [`NOTE_LINES`] lines and shortened with
/// "…" if longer. Rows are always fully readable before they are tapped,
/// so a description never needs a tap that would change a setting.
/// Without a note, the row looks like [`draw_value_row`].
#[allow(clippy::too_many_arguments)]
pub fn draw_noted_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    title: &str,
    value: &str,
    note: &str,
) {
    if note.is_empty() {
        return draw_value_row(canvas, env, style, rect, focused, title, value);
    }
    let band = title_area(style, rect);
    draw_value_row(canvas, env, style, band, focused, title, value);
    draw_note(canvas, env, style, rect, focused, note);
}

/// Draws the note of a noted row below its [`title_area`].
pub fn draw_note(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    note: &str,
) {
    let below = title_area(style, rect).bottom() as u16;
    draw_note_below(canvas, env, style, rect, below, focused, note);
}

/// Draws the note of a row from `below` down to the end of `rect`: in
/// small gray text (light on a focused row), wrapped to [`NOTE_LINES`].
pub(super) fn draw_note_below(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    below: u16,
    focused: bool,
    note: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let at = Rect::new(
        rect.x,
        below,
        rect.width,
        rect.bottom().saturating_sub(u32::from(below)) as u16,
    );
    canvas.describe(Role::Text, note, at, false, None);
    let margin = style.margin as f32;
    let width = f32::from(rect.width) - 2.0 * margin;
    let color = style.focus_faint(focused, 80);
    let line = style.small * NOTE_LINE_HEIGHT;
    let ascent = font.line_metrics(style.small).ascent;
    let top = f32::from(below);
    let lines = font.wrap(style.small, note, width);
    for (n, range) in lines.iter().take(NOTE_LINES).enumerate() {
        let last = n + 1 == NOTE_LINES && lines.len() > NOTE_LINES;
        let text = if last {
            fit(&font, style.small, note[range.start..].trim_end(), width)
        } else {
            note[range.clone()].to_string()
        };
        let baseline = top + line * n as f32 + ascent;
        font.draw(
            canvas,
            style.small,
            f32::from(rect.x) + margin,
            baseline as i32,
            &text,
            color,
        );
    }
}

/// Draws a row: `title` on the left, `value` right-aligned.
#[allow(clippy::too_many_arguments)]
pub fn draw_value_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    title: &str,
    value: &str,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let color = style.focus_ink(focused);
    let margin = style.margin as f32;
    let width = f32::from(rect.width) - 2.0 * margin;
    // At least a margin between them: the value ends at the margin, as
    // the title begins at it.
    let (title, value, value_width) = fit_pair(&font, style.text, title, value, width, margin);
    let value_x = f32::from(rect.x) + f32::from(rect.width) - margin - value_width;
    draw_label(canvas, &font, style.text, value_x, rect, &value, color);
    let title_x = f32::from(rect.x) + margin;
    draw_label(canvas, &font, style.text, title_x, rect, &title, color);
}

/// Room a number's value always gets between its − and +, so the
/// buttons of rows above each other line up.
pub(super) const STEPPER_VALUE: &str = "000 %";

/// The − and + buttons of a number row, with room for `room` between
/// them (at least for "000 %"): the value, or the widest of the
/// numbers in rows above each other (see [`number_room`]), so their
/// buttons line up.
pub fn stepper(env: &Env<'_>, style: &UiStyle, rect: Rect, room: &str) -> Option<(Rect, Rect)> {
    let font = style.font(env)?;
    let size = button_height(rect);
    let y = rect.y + (rect.height - size) / 2;
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let right = rect.x + rect.width - margin;
    let plus = Rect::new(right.saturating_sub(size), y, size, size);
    let value_width = font
        .measure(style.text, room)
        .max(font.measure(style.text, STEPPER_VALUE)) as u16;
    let minus_x = plus.x.saturating_sub(2 * gap + value_width + size);
    Some((Rect::new(minus_x, y, size, size), plus))
}

/// The widest value the numbers among `specs` show, from their least to
/// their greatest, with their units (and the mark of a value set for a
/// scope): the room of their steppers (see [`stepper`]) where they are
/// listed above each other.
pub fn number_room<'a>(
    env: &Env<'_>,
    style: &UiStyle,
    specs: impl IntoIterator<Item = &'a SettingSpec>,
) -> String {
    let Some(font) = style.font(env) else {
        return String::new();
    };
    let mut widest = (0.0, String::new());
    for spec in specs {
        let SettingKind::Int { min, max } = spec.kind else {
            continue;
        };
        for v in [min, max] {
            let text = match &spec.extra().unit {
                Some(unit) => env.i18n.plural(unit, v, &[]),
                None => format!("{v}"),
            };
            let text = if spec.scoped {
                format!("• {text}")
            } else {
                text
            };
            let width = font.measure(style.text, &text);
            if width > widest.0 {
                widest = (width, text);
            }
        }
    }
    widest.1
}

/// A number on a light row: its title, then `[−] value [+]` exactly as in
/// menus, with `room` for the value (see [`stepper`]); `focus` frames the
/// `−` (`Some(false)`) or the `+` (`Some(true)`).
#[allow(clippy::too_many_arguments)]
pub fn draw_number(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    title: &str,
    value: &str,
    room: &str,
    focus: Option<bool>,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let room = wider(&font, style.text, room, value);
    let Some((minus, plus)) = stepper(env, style, rect, room) else {
        return;
    };
    let margin = style.margin as f32;
    // From the row's own start, e.g. right of an icon.
    let left = f32::from(rect.x) + margin;
    let title = fit(&font, style.text, title, f32::from(minus.x) - left - margin);
    draw_label(canvas, &font, style.text, left, rect, &title, Color::BLACK);
    let value_x = centred(&font, style.text, value, minus, plus);
    draw_label(
        canvas,
        &font,
        style.text,
        value_x,
        rect,
        value,
        Color::BLACK,
    );
    draw_button(canvas, env, style, minus, "−", false, focus == Some(false));
    draw_button(canvas, env, style, plus, "+", false, focus == Some(true));
}

/// The wider of `room` and `value` as drawn: the room a stepper keeps for
/// its value, so that steppers of stacked rows line up while no value is
/// wider than that room (measured, not counted in bytes: "−" or "•" take
/// three bytes and are narrow).
pub(super) fn wider<'a>(
    font: &FontHandle<'_>,
    size: f32,
    room: &'a str,
    value: &'a str,
) -> &'a str {
    if font.measure(size, room) < font.measure(size, value) {
        value
    } else {
        room
    }
}

/// Where `value` starts, centred between the buttons `minus` and `plus`.
pub(super) fn centred(
    font: &FontHandle<'_>,
    size: f32,
    value: &str,
    minus: Rect,
    plus: Rect,
) -> f32 {
    let left = f32::from(minus.x + minus.width);
    let room = f32::from(plus.x) - left;
    left + (room - font.measure(size, value)) / 2.0
}

/// A focused number: title, then `[−] value [+]`, with `room` for the
/// value (see [`stepper`]). While editing with buttons the boxes are
/// filled.
#[allow(clippy::too_many_arguments)]
pub fn draw_stepper(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    title: &str,
    value: &str,
    room: &str,
    editing: bool,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let room = wider(&font, style.text, room, value);
    let Some((minus, plus)) = stepper(env, style, rect, room) else {
        return;
    };
    let margin = style.margin as f32;
    // From the row's own start, e.g. right of an icon.
    let left = f32::from(rect.x) + margin;
    let title = fit(&font, style.text, title, f32::from(minus.x) - left - margin);
    let ink = style.focus_ink(true);
    draw_label(canvas, &font, style.text, left, rect, &title, ink);
    let value_x = centred(&font, style.text, value, minus, plus);
    draw_label(canvas, &font, style.text, value_x, rect, value, ink);
    draw_focused_steps(canvas, style, &font, rect, (minus, plus), editing);
}

/// The `−` and `+` of a focused number, on the focus bar in `row`:
/// outlined in white, or filled white while the buttons change it
/// (`editing`).
pub(super) fn draw_focused_steps(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    font: &FontHandle<'_>,
    row: Rect,
    (minus, plus): (Rect, Rect),
    editing: bool,
) {
    for (button, glyph) in [(minus, "−"), (plus, "+")] {
        let (x, y, w, h) = (
            i32::from(button.x),
            i32::from(button.y),
            u32::from(button.width),
            u32::from(button.height),
        );
        let ink = style.focus_ink(true);
        let paper = style.focus_paper(true);
        let r = style.radius(h);
        let glyph_color = if editing {
            canvas.fill_round_rect(x, y, w, h, r, ink);
            paper
        } else {
            canvas.stroke_round_rect(x, y, w, h, button_border(style), r, ink);
            ink
        };
        let glyph_width = font.measure(style.text, glyph);
        let glyph_x = f32::from(button.x) + (f32::from(button.width) - glyph_width) / 2.0;
        draw_label(canvas, font, style.text, glyph_x, row, glyph, glyph_color);
    }
}

/// The value of a setting for display, with its unit.
pub fn setting_value(env: &Env<'_>, spec: &SettingSpec) -> String {
    let value = plain_setting_value(env, spec);
    // Set for the scope (e.g. this book only): marked.
    if env.settings.is_scoped(spec.id.as_str()) {
        format!("• {value}")
    } else {
        value
    }
}

pub(super) fn plain_setting_value(env: &Env<'_>, spec: &SettingSpec) -> String {
    match env.settings.get(spec.id.as_str()) {
        Some(SettingValue::Bool(true)) => env.i18n.tr("core.on").to_string(),
        Some(SettingValue::Bool(false)) => env.i18n.tr("core.off").to_string(),
        Some(SettingValue::Int(v)) => match &spec.extra().unit {
            Some(unit) => env.i18n.plural(unit, *v, &[]),
            None => format!("{v}"),
        },
        Some(SettingValue::Text(v)) if spec.extra().secret && !v.is_empty() => "••••••".to_string(),
        Some(SettingValue::Text(v)) => option_name(env, spec, v),
        None => String::new(),
    }
}

/// The display name of a choice or language value.
pub fn option_name(env: &Env<'_>, spec: &SettingSpec, value: &str) -> String {
    match spec.kind {
        SettingKind::Language => env
            .i18n
            .languages()
            .into_iter()
            .find(|(tag, _)| *tag == value)
            .map_or(value.to_string(), |(_, name)| name.to_string()),
        // Choices are ids; their translation is their display name.
        SettingKind::Choice(_) => env.i18n.tr(value).to_string(),
        SettingKind::List => {
            let names: Vec<&str> = crate::status::item_ids(value)
                .map(|id| {
                    let title = env.status_items.title(id).or_else(|| env.widgets.title(id));
                    env.i18n.tr(title.unwrap_or(id))
                })
                .collect();
            if names.is_empty() {
                env.i18n.tr("core.list.none").to_string()
            } else {
                names.join(", ")
            }
        }
        _ => value.to_string(),
    }
}

/// Whether a choice (or the language) has just two options: choosing it
/// switches to the other at once, as a switch does, instead of opening a
/// list that would only offer that one (fewer presses).
pub fn two_options(env: &Env<'_>, spec: &SettingSpec) -> bool {
    // More ways to choose are listed with the options.
    if !env.menus.items(spec.id.as_str()).is_empty() {
        return false;
    }
    match &spec.kind {
        SettingKind::Choice(options) => options.len() == 2,
        SettingKind::Language => env.i18n.languages().len() == 2,
        _ => false,
    }
}

/// What choosing a setting (confirm or a tap) opens, the same on every
/// screen that lists settings: its editor ([`SettingExtra::editor`](crate::SettingExtra::editor)),
/// the keyboard for text (a secret entered afresh), the values of a
/// choice of more than two on the screen of slot
/// [`CHOOSE_VALUE`](crate::builtin::slots::CHOOSE_VALUE). `None` where
/// the screen changes it itself: a switch, a number, a choice of two
/// (see [`next_value`]), or a choice where no screen fills that slot.
pub fn setting_opens(env: &Env<'_>, spec: &SettingSpec) -> Option<crate::Command> {
    if let Some(editor) = &spec.extra().editor {
        return Some(editor.open(&spec.id));
    }
    match spec.kind {
        SettingKind::Text => {
            let question = crate::ask::text(
                env.i18n.tr(&spec.title),
                &crate::builtin::events::SETTING_TYPED,
            )
            .with(crate::builtin::params::ID, spec.id.as_str().to_string());
            let question = if spec.extra().secret {
                question.password()
            } else {
                question.initial(setting_value(env, spec))
            };
            Some(question.command())
        }
        SettingKind::Choice(_) | SettingKind::Language
            if !two_options(env, spec) && env.has_slot(&crate::builtin::slots::CHOOSE_VALUE) =>
        {
            Some(crate::Editor::Slot(crate::builtin::slots::CHOOSE_VALUE).open(&spec.id))
        }
        _ => None,
    }
}

/// Whether choosing the setting opens another screen (its own editor, or
/// a list to choose from): its row shows a `›`.
pub fn opens_screen(env: &Env<'_>, spec: &SettingSpec) -> bool {
    spec.extra().editor.is_some()
        || (matches!(spec.kind, SettingKind::Choice(_) | SettingKind::Language)
            && !two_options(env, spec))
}

/// The value after one step of the setting: a switch toggles, a number
/// moves by `step` within its range, a choice or language moves to the
/// next (or previous) option. `None` for text.
pub fn next_value(env: &Env<'_>, spec: &SettingSpec, step: i64) -> Option<SettingValue> {
    let current = env.settings.get(spec.id.as_str())?;
    let cycle = |options: &[&str], value: &str| -> SettingValue {
        let len = options.len().max(1) as i64;
        // From a value not among the options, the first step on is the
        // first option and the first step back the last.
        let index = match options.iter().position(|o| *o == value) {
            Some(index) => index as i64,
            None if step > 0 => -1,
            None => len,
        };
        let next = options
            .get(index.saturating_add(step).rem_euclid(len) as usize)
            .copied()
            .unwrap_or_default();
        SettingValue::Text(Cow::Owned(next.to_string()))
    };
    Some(match (&spec.kind, current) {
        (SettingKind::Bool, SettingValue::Bool(v)) => SettingValue::Bool(!v),
        (SettingKind::Int { min, max }, SettingValue::Int(v)) => {
            let next = v.saturating_add(step.saturating_mul(spec.extra().step));
            SettingValue::Int(next.clamp(*min, (*max).max(*min)))
        }
        (SettingKind::Choice(options), SettingValue::Text(v)) => {
            let options: Vec<&str> = options.iter().collect();
            cycle(&options, v)
        }
        (SettingKind::Language, SettingValue::Text(v)) => {
            let tags: Vec<&str> = env.i18n.languages().into_iter().map(|(t, _)| t).collect();
            cycle(&tags, v)
        }
        _ => return None,
    })
}
