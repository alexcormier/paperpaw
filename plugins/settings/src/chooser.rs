//! Choosing one of a setting's options, on a page of its own.

use super::*;

// ---- Choosing a value ---------------------------------------------------

#[derive(Debug)]
pub(super) struct Chooser {
    pub(super) spec: Option<SettingSpec>,
    /// (value, display name)
    pub(super) options: Vec<(String, String)>,
    pub(super) style: UiStyle,
    pub(super) header: ui::Header,
    pub(super) list: ListView,
}

impl Chooser {
    pub(super) fn new(env: &Env<'_>, id: &str) -> Self {
        let (style, header, mut list) = frame(env, false);
        // Of a placement too (`widgets.clock@2.face`), as that one.
        let spec = env.settings.spec(id).cloned().map(|mut spec| {
            if let Ok(id) = Id::parse(id) {
                spec.id = id;
            }
            spec
        });
        let options: Vec<(String, String)> = match spec.as_ref().map(|s| (&s.kind, s)) {
            Some((SettingKind::Language, _)) => env
                .i18n
                .languages()
                .into_iter()
                .map(|(tag, name)| (tag.to_string(), name.to_string()))
                .collect(),
            Some((SettingKind::Choice(values), spec)) => values
                .iter()
                .map(|v| (v.to_string(), ui::option_name(env, spec, v)))
                .collect(),
            _ => Vec::new(),
        };
        let current = env.settings.text(id);
        list.focus = options.iter().position(|(v, _)| v == current).unwrap_or(0);
        Self {
            spec,
            options,
            style,
            header,
            list,
        }
    }

    pub(super) fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let (Some(spec), Some((value, _))) = (&self.spec, self.options.get(index)) else {
            return;
        };
        cx.command(Command::Back);
        cx.command(Command::SetSetting(
            spec.id.clone(),
            SettingValue::Text(Cow::Owned(value.clone())),
        ));
    }
}

impl Screen for Chooser {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.options.len();
        let title = self
            .spec
            .as_ref()
            .map_or("", |spec| env.i18n.tr(&spec.title));
        self.header
            .draw(canvas, env, &style, title, None, self.list.paging(len));
        let options = &self.options;
        let current = self
            .spec
            .as_ref()
            .map_or("", |spec| env.settings.text(spec.id.as_str()));
        self.list
            .draw(canvas, &style, len, |canvas, index, rect, focused| {
                let (value, name) = &options[index];
                let mark = if value == current { "•" } else { "" };
                ui::draw_value_row(canvas, env, &style, rect, focused, name, mark);
            });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.options.len();
        if let Some(change) = self.list.navigate(action, len) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.choose(self.list.focus, cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.options.len());
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging);
        if let Some(action) = tapped {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, self.options.len()) {
            Some(index) => {
                self.choose(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}
