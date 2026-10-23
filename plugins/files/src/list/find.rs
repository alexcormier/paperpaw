//! Finding a book without a keyboard: the letters of the alphabet, each
//! listing the books whose title (without "The") or author's surname
//! starts with it. Letters no book starts with are faint and skipped.
//! Above them, searching with the keyboard (on every device; with
//! buttons alone it is the slower way).
//!
//! Which letters have books is found in one pass over the catalog when it
//! opens.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

use paperpaw_sdk::library;

use super::index::{letter, sort_key, surname};
use super::shelf::{self, Kind};

/// The letters, `#` for titles starting otherwise.
const LETTERS: &[u8; 27] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ#";

pub(crate) struct FindScreen {
    style: UiStyle,
    header: ui::Header,
    /// The row that searches with the keyboard.
    search: Rect,
    /// The letters some book starts with, and their cells.
    letters: Vec<u8>,
    cells: Vec<Rect>,
    columns: usize,
    /// The note below the letters.
    note: Rect,
    /// 0 is the search, then the letters.
    focus: usize,
    shown: bool,
}

impl FindScreen {
    pub(crate) fn new(env: &Env<'_>, _: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let row = style.touch_row(style.row) as u16;
        let search = Rect::new(0, header.area().height + gap, display.width, row);
        // Which letters have books: by title (without "The") or surname.
        let mut has = [false; 27];
        library::service(env).each(env, &mut |_, line| {
            let by_title = match sort_key(&line.title)[0] {
                b @ b'a'..=b'z' => b.to_ascii_uppercase(),
                _ => b'#',
            };
            let by_author = letter(surname(&line.author));
            for (n, l) in LETTERS.iter().enumerate() {
                has[n] |= *l == by_title || (*l == by_author && by_author != b'#');
            }
        });
        let letters: Vec<u8> = LETTERS
            .iter()
            .zip(has)
            .filter_map(|(l, has)| has.then_some(*l))
            .collect();
        // Cells about 11 mm wide, whole columns within the margins.
        let width = display.width.saturating_sub(2 * margin);
        let columns = (u32::from(width) / env.metrics.mm(11).max(1)).clamp(5, 8) as u16;
        let cell = width / columns;
        let top = search.y + search.height + gap;
        let cells: Vec<Rect> = (0..letters.len() as u16)
            .map(|n| {
                Rect::new(
                    margin + (n % columns) * cell,
                    top + (n / columns) * cell,
                    cell,
                    cell,
                )
            })
            .collect();
        let bottom = cells.last().map_or(top, |c| c.y + c.height);
        let note = Rect::new(0, bottom + gap, display.width, row * 2);
        Self {
            focus: usize::from(!letters.is_empty()),
            shown: !env.device.input.touch,
            columns: usize::from(columns),
            style,
            header,
            search,
            letters,
            cells,
            note,
        }
    }

    /// Moves the focus `delta` places (a row of letters for the page
    /// buttons); the search is before the first letter.
    fn step(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        if !self.shown {
            self.shown = true;
        } else {
            let len = self.letters.len() as isize + 1;
            self.focus = (self.focus as isize + delta).rem_euclid(len) as usize;
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        if index == 0 {
            cx.command(Command::RunAction(crate::SEARCH));
        } else if let Some(letter) = self.letters.get(index - 1) {
            let letter = String::from(*letter as char);
            cx.command(shelf::open(Kind::Letter, &letter));
        }
    }
}

impl Screen for FindScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n.tr("files.list.find.title");
        self.header.draw(canvas, env, &style, title, None, None);
        let Some(font) = style.font(env) else {
            return;
        };
        // The search, as a field.
        let focused = self.shown && self.focus == 0;
        if focused {
            ui::draw_focus(canvas, &style, self.search);
        } else {
            let field = ui::focus_rect(&style, self.search);
            let radius = style.radius(u32::from(field.height));
            canvas.stroke_round(field, style.line.max(1), radius, Color::BLACK);
        }
        let ink = style.focus_ink(focused);
        let margin = style.margin as f32;
        let size = style.text * ui::ICON_SCALE;
        let w = ui::draw_icon(canvas, env, "search", size, margin, self.search, ink);
        let x = margin + w + style.gap as f32;
        let hint = env.i18n.tr("files.list.find.search");
        let faint = style.focus_faint(focused, ui::Ink::Detail);
        ui::draw_label(canvas, &font, style.text, x, self.search, hint, faint);
        // The letters, the focused one on a dark circle.
        let big = style.title * 1.2;
        for (n, (cell, letter)) in self.cells.iter().zip(&self.letters).enumerate() {
            let focused = self.shown && self.focus == n + 1;
            let ink = if focused {
                let inner = cell.inset(style.gap as u16 / 3, style.gap as u16 / 3);
                canvas.fill_round(inner, u32::from(inner.height / 2), Color::BLACK);
                Color::WHITE
            } else {
                Color::BLACK
            };
            let label = [*letter];
            let text = core::str::from_utf8(&label).unwrap_or("#");
            let w = font.measure(big, text);
            let x = f32::from(cell.x) + (f32::from(cell.width) - w) / 2.0;
            ui::draw_label(canvas, &font, big, x, *cell, text, ink);
        }
        let note = ui::tr_input(env, "files.list.find.note");
        ui::draw_message(canvas, env, &style, self.note, note);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let columns = self.columns as isize;
        if [actions::LEFT, actions::PREVIOUS].contains(a) {
            self.step(-1, cx);
        } else if [actions::RIGHT, actions::NEXT].contains(a) {
            self.step(1, cx);
        } else if [actions::UP, actions::PREV_PAGE].contains(a) {
            self.step(-columns, cx);
        } else if [actions::DOWN, actions::NEXT_PAGE].contains(a) {
            self.step(columns, cx);
        } else if *a == actions::CONFIRM {
            self.choose(self.focus, cx);
        } else if *a == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let point = gesture.start;
        if let Some(action) = self.header.tap(cx.env(), &self.style, point, None) {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        if self.search.contains(point) {
            self.choose(0, cx);
        } else if let Some(n) = self.cells.iter().position(|c| c.contains(point)) {
            self.choose(n + 1, cx);
        }
        Handled::Yes
    }
}
