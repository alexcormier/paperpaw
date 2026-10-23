//! Every tool at once, as icons in a grid (as a phone lists its apps):
//! opened by swiping up on the home screen. Tapping one runs it; the
//! buttons move between them. And the hint on the gestures, once, the
//! first time the home screen shows on a touch screen.

use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::widgets::grid;
use paperpaw_sdk::{Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Rect, Screen};

use crate::{blocks, pages};

/// Columns of the overview.
const COLUMNS: u32 = 4;

pub(crate) struct DrawerScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    tools: Vec<Id>,
    /// The lines the longest name takes: every tool keeps room for them.
    lines: usize,
}

impl DrawerScreen {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let top = header.area.height + style.gap as u16;
        let area = Rect::new(
            style.margin as u16,
            top,
            display.width.saturating_sub(2 * style.margin as u16),
            display.height.saturating_sub(top),
        );
        // About as high as a column is wide; a line higher where a tool's
        // name takes two (its icon as large as the others').
        let row = (u32::from(area.width) / COLUMNS * 4 / 5).max(style.row);
        let tools: Vec<Id> = pages::tools(env).into_iter().map(|(id, _)| id).collect();
        let cell = Rect::new(0, 0, (u32::from(area.width) / COLUMNS) as u16, row as u16);
        let lines = tools
            .iter()
            .map(|id| blocks::tool_name_lines(env, cell, id))
            .max()
            .unwrap_or(0);
        let more = if lines > 1 {
            u32::from(paperpaw_sdk::widgets::line_height(env, style.small))
        } else {
            0
        };
        Self {
            list: ListView::new(area, row + more)
                .columns(COLUMNS)
                .focus_on_keys(env),
            tools,
            lines,
            style,
            header,
        }
    }

    fn run(&self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some(action) = self.tools.get(index) {
            cx.command(Command::Back);
            cx.command(Command::RunAction(action.clone()));
        }
    }
}

impl Screen for DrawerScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.tools.len();
        self.header.draw(
            canvas,
            env,
            &style,
            env.i18n.tr("home.drawer"),
            None,
            self.list.paging(len),
        );
        let tools = &self.tools;
        let list = &self.list;
        let first = list.page() * list.rows();
        // Room for as many lines of a name as the longest takes, so that
        // icons and names line up.
        let lines = self.lines;
        for (slot, index) in (first..len).take(list.rows()).enumerate() {
            let rect = list.row_rect(slot);
            let focused = list.shown() && index == list.focus();
            // Named after its action, also where its name is not shown.
            let name = grid::What::Tool(tools[index].clone()).title(env);
            canvas.describe(
                paperpaw_sdk::semantics::Role::Button,
                &name,
                rect,
                focused,
                None,
            );
            let ink = blocks::draw_tool_lines(canvas, env, rect, &tools[index], lines);
            if focused {
                // Round its icon and name, never through them.
                paperpaw_sdk::widgets::grid::draw_tool_focus(canvas, &style, ink);
            }
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.tools.len();
        let a = action;
        let columns = COLUMNS as isize;
        let delta = if *a == actions::LEFT || *a == actions::PREVIOUS {
            -1
        } else if *a == actions::RIGHT || *a == actions::NEXT {
            1
        } else if *a == actions::UP {
            -columns
        } else if *a == actions::DOWN {
            columns
        } else if *a == actions::NEXT_PAGE {
            self.list.rows() as isize
        } else if *a == actions::PREV_PAGE {
            -(self.list.rows() as isize)
        } else if *a == actions::CONFIRM {
            self.run(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list.move_focus(delta, len).request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        match gesture.kind {
            GestureKind::Tap => {
                let len = self.tools.len();
                let paging = self.list.paging(len);
                if let Some(action) = self
                    .header
                    .tap(cx.env(), &self.style, gesture.start, paging)
                {
                    cx.command(Command::RunAction(action));
                    return Handled::Yes;
                }
                match self.list.hit(gesture.start, len) {
                    Some(index) => {
                        self.run(index, cx);
                        Handled::Yes
                    }
                    None => Handled::No,
                }
            }
            // Down again: back to the home screen.
            GestureKind::Swipe(paperpaw_sdk::Direction::Down) => {
                cx.command(Command::Back);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}
