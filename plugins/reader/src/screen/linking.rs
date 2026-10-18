//! Choosing a link on the page with the buttons, following it, and the
//! way back.

use super::*;

/// Choosing a link with the buttons: which of the page's links that can
/// be followed has the focus (`None` if the page has none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Linking {
    pub(super) focus: Option<usize>,
}

impl ReaderScreen {
    pub(super) fn set_linking(&mut self, to: Option<Linking>, cx: &mut Cx<'_, '_>) {
        if self.linking != to {
            self.linking = to;
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    /// Buttons while choosing a link: the next or previous one (on to
    /// the next or previous page past the last or first), *confirm*
    /// follows it, *back* ends.
    pub(super) fn on_linking(
        &mut self,
        action: &Id,
        linking: Linking,
        cx: &mut Cx<'_, '_>,
    ) -> Handled {
        let env = *cx.env();
        let a = action;
        let forward = [
            actions::RIGHT,
            actions::DOWN,
            actions::NEXT,
            actions::NEXT_PAGE,
        ];
        let backward = [
            actions::LEFT,
            actions::UP,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ];
        let Ok(book) = &mut self.book else {
            self.linking = None;
            return Handled::No;
        };
        let count = book.followable(&env).len();
        if forward.contains(a) {
            match linking.focus {
                Some(i) if i + 1 < count => {
                    self.set_linking(Some(Linking { focus: Some(i + 1) }), cx);
                }
                _ => {
                    let end = book.page.end;
                    book.show(cx, end, true);
                    let focus = (!book.followable(&env).is_empty()).then_some(0);
                    self.linking = Some(Linking { focus });
                    cx.request_redraw();
                }
            }
        } else if backward.contains(a) {
            match linking.focus {
                Some(i) if i > 0 => {
                    self.set_linking(Some(Linking { focus: Some(i - 1) }), cx);
                }
                _ => {
                    if let Some(start) = book.previous_start(&env) {
                        book.show(cx, start, false);
                    }
                    let count = book.followable(&env).len();
                    self.linking = Some(Linking {
                        focus: count.checked_sub(1),
                    });
                    cx.request_redraw();
                }
            }
        } else if *a == actions::CONFIRM {
            let link = linking
                .focus
                .and_then(|i| book.followable(&env).get(i).copied())
                .map(|i| book.links[i].clone());
            let link = link.map(|l| {
                let words = book.words(&env);
                (l.href.clone(), link_label(book, &words, &l))
            });
            self.set_linking(None, cx);
            if let Some((href, label)) = link {
                return self.open_link(&href, &label, cx);
            }
        } else if *a == actions::BACK || *a == actions::MENU {
            self.set_linking(None, cx);
        } else {
            // Anything else ends choosing and goes on as usual.
            self.set_linking(None, cx);
            return Handled::No;
        }
        Handled::Yes
    }

    /// Opens a link: a note over the page if it looks like one and the
    /// reader shows notes that way, else follows it.
    pub(super) fn open_link(&mut self, href: &str, label: &str, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let popup =
            env.settings.read(&crate::settings::FOOTNOTES) == crate::settings::FOOTNOTES_POPUP;
        if popup
            && !quireink_sdk::document::is_external(href)
            && crate::footnote::note_like(label)
            && let Ok(book) = &self.book
            && let Some(text) = book.note_text(&env, href)
        {
            cx.command(Command::OpenScreen(
                crate::FOOTNOTE_SCREEN,
                Params::new()
                    .with(params::TEXT, text)
                    .with(params::PATH, book.path.clone())
                    .with(crate::footnote::HREF, href.to_string()),
            ));
            return Handled::Yes;
        }
        self.follow(href, cx)
    }

    /// Follows link `href`: to its place in the book, remembering where
    /// it was followed from for *Back*.
    pub(super) fn follow(&mut self, href: &str, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let Ok(book) = &mut self.book else {
            return Handled::No;
        };
        if quireink_sdk::document::is_external(href) {
            // To the web, if a browser is there.
            if !env.has_slot(&slots::BROWSER) {
                return Handled::No;
            }
            // Told apart by the time, so a page asked for again is loaded
            // again rather than the open browser just brought forward.
            cx.command(Command::OpenSlot(
                slots::BROWSER,
                Params::new()
                    .with(params::URL, href.to_string())
                    .with(params::ID, format!("{}", env.now.0)),
            ));
            return Handled::Yes;
        }
        let Some(target) = book.document.link_target(env.storage, href) else {
            return Handled::No;
        };
        let start = book.page_start(&env, target);
        book.jump(cx, start);
        Handled::Yes
    }
}

/// The text of `link` on the page: the parts of its words inside it (a
/// note mark is often part of a word, as in `source¹`).
pub(super) fn link_label(book: &Book, words: &[WordBox], link: &Link) -> String {
    let mut label = String::new();
    for word in words.iter().filter(|w| {
        w.start.section == book.page.start.section
            && w.end.offset > link.start
            && w.start.offset < link.end
    }) {
        let from = link.start.saturating_sub(word.start.offset) as usize;
        let to = (link.end.min(word.end.offset) - word.start.offset) as usize;
        let part = word.text.get(from..to).unwrap_or(&word.text);
        if !label.is_empty() {
            label.push(' ');
        }
        label.push_str(part);
    }
    label
}

/// The link being chosen, framed like selected text, and a bar at the
/// bottom saying where it leads and what the buttons do.
pub(super) fn draw_linking(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    book: &Book,
    words: &[WordBox],
    linking: Linking,
) {
    let link = linking
        .focus
        .and_then(|i| book.followable(env).get(i).copied())
        .map(|i| &book.links[i]);
    if let Some(link) = link {
        let line = env.metrics.tenth_mm(4).max(2);
        let inside = |i: usize| {
            words[i].start.section == book.page.start.section
                && words[i].end.offset > link.start
                && words[i].start.offset < link.end
        };
        for b in selection::line_boxes(words, inside) {
            canvas.stroke_rect(
                i32::from(b.x) - 4,
                i32::from(b.y) - 2,
                u32::from(b.width) + 8,
                u32::from(b.height) + 4,
                line,
                Color::BLACK,
            );
        }
    }
    let style = UiStyle::new(env);
    let bar = selection::bar_area(env, &style);
    ui::fill(canvas, bar, Color::WHITE);
    canvas.fill_rect(
        i32::from(bar.x),
        i32::from(bar.y),
        u32::from(bar.width),
        style.line.max(2),
        Color::BLACK,
    );
    let Some(font) = style.font(env) else {
        return;
    };
    let text = match link {
        Some(link) => {
            let place = if quireink_sdk::document::is_external(&link.href) {
                quireink_sdk::web::host(&link.href).to_string()
            } else {
                env.i18n.tr("reader.link.in-book").to_string()
            };
            format!("{place} · {}", env.i18n.tr("reader.link.hint"))
        }
        None => env.i18n.tr("reader.link.none").to_string(),
    };
    let margin = style.margin as f32;
    let text = ui::fit(
        &font,
        style.small,
        &text,
        f32::from(bar.width) - 2.0 * margin,
    );
    ui::draw_label(
        canvas,
        &font,
        style.small,
        f32::from(bar.x) + margin,
        bar,
        &text,
        Color::BLACK,
    );
}
