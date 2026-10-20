//! One font: a sentence set in it, what it is and its licence, and what
//! can be done with it: download it, read with it, set the status lines
//! in it, update or remove it.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, settings};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::Progress;
use paperpaw_sdk::ui::choice::{self, Picked};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen, SettingValue, files,
};

use crate::catalog::Font;
use crate::install::{self, DOWNLOADED, INSTALLER};
use crate::samples::{self, SAMPLE, Samples};
use crate::{CHOICE, list};

/// Parameter of a font's page: the font ([`Font::to_param`]), as the
/// list has it.
pub(crate) const FONT: &str = "font";

/// The answer to *Remove?*.
const REMOVE: Event<Picked> = Event::new("fonts-browser.remove");

/// Lines of the sample at most.
const SAMPLE_LINES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Download,
    Stop,
    Read,
    Status,
    Update,
    Remove,
}

pub(crate) struct FontScreen {
    style: UiStyle,
    header: ui::Header,
    /// Where the sample and what the font is go.
    sample: Rect,
    info: Rect,
    list: ListView,
    status: Rect,
    font: Option<Font>,
    samples: Samples,
    /// The download going on, kept here: drawing reads nothing from the
    /// storage.
    job: Option<Progress>,
}

impl FontScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let samples = Samples::new(&style);
        let display = env.device.display;
        let gap = style.gap as u16;
        let top = header.area.height + gap;
        let line = (style.row as u16) * 3 / 2;
        let status = Rect::new(0, display.height - line, display.width, line);
        let sample_line = (samples.size as f32 * 1.4) as u16;
        let sample = Rect::new(0, top, display.width, sample_line * SAMPLE_LINES as u16);
        let info_height = (style.small * 1.4 * 2.0) as u16 + gap;
        let info = Rect::new(
            0,
            sample.y + sample.height + gap,
            display.width,
            info_height,
        );
        let rows_top = info.y + info.height + gap;
        let area = Rect::new(
            0,
            rows_top,
            display.width,
            status.y.saturating_sub(rows_top),
        );
        let font = params.get(FONT).and_then(Font::from_param);
        Self {
            list: ListView::new(area, style.row).focus_on_keys(env),
            sample,
            info,
            status,
            font,
            samples,
            job: INSTALLER.progress(env.storage),
            style,
            header,
        }
    }

    fn rows(&self, env: &Env<'_>) -> Vec<Row> {
        let Some(font) = &self.font else {
            return Vec::new();
        };
        let installing = self.job.as_ref().is_some_and(|j| j.plugin == font.plugin());
        if installing {
            return alloc::vec![Row::Stop];
        }
        if font.installed(env).is_none() {
            return alloc::vec![Row::Download];
        }
        let mut rows = alloc::vec![Row::Read, Row::Status];
        if font.newer(env) {
            rows.push(Row::Update);
        }
        rows.push(Row::Remove);
        rows
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn want_sample(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let mut out = Vec::new();
        if self.samples.want(&env, self.font.as_slice(), &mut out) {
            self.redraw(cx);
        }
        for command in out {
            cx.command(command);
        }
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(font) = self.font.clone() else {
            return;
        };
        let name = || SettingValue::Text(font.name.clone().into());
        match self.rows(&env).get(index).copied() {
            Some(Row::Download | Row::Update) => {
                if !env.caps.network {
                    let text = env.i18n.tr("fonts-browser.offline").to_string();
                    cx.command(Command::Notify(text));
                    return;
                }
                let mut out = Vec::new();
                install::start(&env, &font, &mut out);
                for command in out {
                    cx.command(command);
                }
                self.job = INSTALLER.progress(env.storage);
                self.list.set_focus(0);
                self.redraw(cx);
            }
            Some(Row::Stop) => {
                INSTALLER.cancel(env.storage);
                self.job = None;
                self.redraw(cx);
            }
            Some(Row::Read) => {
                cx.command(Command::SetSetting(
                    settings::READING_FONT.id().clone(),
                    name(),
                ));
            }
            Some(Row::Status) => {
                cx.command(Command::SetSetting(
                    settings::STATUS_FONT.id().clone(),
                    name(),
                ));
            }
            Some(Row::Remove) => {
                let title = env.i18n.format(
                    "fonts-browser.remove-question",
                    &[("name", Arg::Str(&font.name))],
                );
                cx.command(choice::open(
                    CHOICE,
                    &REMOVE,
                    &title,
                    &font.plugin(),
                    "fonts-browser.option.",
                    &["remove", "keep"],
                ));
            }
            None => {}
        }
    }

    /// A row's name and what it says at its end.
    fn row_text(env: &Env<'_>, job: Option<&Progress>, font: &Font, row: Row) -> (String, String) {
        let name = Arg::Str(&font.name);
        let decimal = env.i18n.tr("fonts-browser.decimal");
        let chosen = |setting: &str| {
            if font.chosen_for(env, setting) {
                String::from("•")
            } else {
                String::new()
            }
        };
        match row {
            Row::Download => (
                env.i18n.tr("fonts-browser.download").to_string(),
                files::size(font.size(), decimal),
            ),
            Row::Stop => {
                let job = job.cloned().unwrap_or_default();
                (
                    env.i18n.tr("fonts-browser.stop").to_string(),
                    format!("{} / {}", job.done, job.total),
                )
            }
            Row::Read => (
                env.i18n
                    .format("fonts-browser.read-with", &[("name", name)]),
                chosen(settings::READING_FONT.as_str()),
            ),
            Row::Status => (
                env.i18n.tr("fonts-browser.status-lines").to_string(),
                chosen(settings::STATUS_FONT.as_str()),
            ),
            Row::Update => (
                env.i18n.tr("fonts-browser.update").to_string(),
                font.version.clone(),
            ),
            Row::Remove => (
                env.i18n.tr("fonts-browser.remove").to_string(),
                String::new(),
            ),
        }
    }
}

impl Screen for FontScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let rows = self.rows(env);
        let title = self.font.as_ref().map_or("", |f| f.name.as_str());
        self.header.draw(
            canvas,
            env,
            &style,
            title,
            None,
            self.list.paging(rows.len()),
        );
        let Some(font) = &self.font else {
            ui::draw_empty(
                canvas,
                env,
                &style,
                self.list.area(),
                "typography",
                env.i18n.tr("fonts-browser.gone"),
                env.i18n.tr("fonts-browser.gone.help"),
            );
            return;
        };
        // The sentence in the font, or in the menus' until it is here.
        let margin = style.margin as f32;
        let width = f32::from(self.sample.width) - 2.0 * margin;
        let sentence = samples::sentence(env);
        let (face, size, color) = match self.samples.get(&font.id) {
            Some(sample) => (Some(sample), self.samples.size as f32, Color::BLACK),
            None => (style.font(env), style.text, style.faint(120)),
        };
        let mut bottom = self.sample.y;
        if let Some(face) = face {
            let line = face.line_metrics(size).line_height;
            let ascent = face.line_metrics(size).ascent;
            let lines = face.wrap(size, sentence, width);
            let shown = lines.len().min(SAMPLE_LINES);
            bottom += (line * shown as f32) as u16;
            for (n, range) in lines.into_iter().take(SAMPLE_LINES).enumerate() {
                let baseline = f32::from(self.sample.y) + line * n as f32 + ascent;
                face.draw(
                    canvas,
                    size,
                    f32::from(self.sample.x) + margin,
                    baseline as i32,
                    sentence[range].trim_end(),
                    color,
                );
            }
        }
        // What it is, by whom, under which licence, right below; then
        // what can be done.
        let gap = style.gap as u16;
        self.info.y = bottom + gap;
        self.list.area().y = self.info.y + self.info.height + gap;
        self.list.area().height = self.status.y.saturating_sub(self.list.area().y);
        if let Some(ui_font) = style.font(env) {
            let line = style.small * 1.4;
            let ascent = ui_font.line_metrics(style.small).ascent;
            let licence = env.i18n.format(
                "fonts-browser.licence",
                &[("licence", Arg::Str(&font.licence))],
            );
            for (n, text) in [list::describe(env, font), licence].iter().enumerate() {
                let text = ui::fit(&ui_font, style.small, text, width);
                let baseline = f32::from(self.info.y) + line * n as f32 + ascent;
                ui_font.draw(
                    canvas,
                    style.small,
                    margin,
                    baseline as i32,
                    &text,
                    style.faint(80),
                );
            }
        }
        let job = self.job.as_ref();
        self.list.draw(
            canvas,
            &style,
            rows.len(),
            |canvas, index, rect, focused| {
                let (name, value) = FontScreen::row_text(env, job, font, rows[index]);
                ui::draw_value_row(canvas, env, &style, rect, focused, &name, &value);
            },
        );
        canvas.fill(self.status, Color::WHITE);
        if rows.first() == Some(&Row::Stop) {
            let note = env.i18n.tr("fonts-browser.downloading");
            ui::draw_note_line(canvas, env, &style, self.status, note);
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.job = INSTALLER.progress(cx.env().storage);
        self.want_sample(cx);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows(cx.env()).len();
        if let Some(change) = self.list.navigate(action, len) {
            self.list.request(change, cx);
        } else if *action == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let len = self.rows(cx.env()).len();
        let paging = self.list.paging(len);
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.activate(index, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(reply) = SAMPLE.read(event, params) {
            let env = *cx.env();
            self.samples.received(&env, &reply, self.font.as_slice());
            self.want_sample(cx);
        } else if let Some(picked) = REMOVE.read(event, params) {
            if picked.option == "remove"
                && let Ok(plugin) = Id::parse(&picked.path)
            {
                cx.command(Command::RemovePlugin(plugin));
                self.list.set_focus(0);
            }
        } else if DOWNLOADED.read(event, params).is_some()
            || *event == events::PLUGINS_CHANGED
            || *event == events::SETTING_CHANGED
        {
            self.job = INSTALLER.progress(cx.env().storage);
            let len = self.rows(cx.env()).len();
            self.list.set_len(len);
            self.redraw(cx);
        }
    }
}
