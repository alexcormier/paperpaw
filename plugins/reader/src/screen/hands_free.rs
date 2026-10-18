//! Reading without turning pages by hand: read aloud sentence by
//! sentence, or the page turned by itself.

use super::*;

/// A sentence read aloud, by words of the page that starts at `page`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Sentence {
    pub(super) page: Locator,
    pub(super) from: usize,
    pub(super) to: usize,
    /// The words of its page.
    pub(super) words: usize,
    /// Words of the following page spoken with it (it goes on there).
    pub(super) carried: usize,
}

/// Reading aloud: the sentence spoken and the one handed over to follow
/// it without a pause.
#[derive(Debug, Clone, Copy)]
pub(super) struct Aloud {
    pub(super) now: Sentence,
    pub(super) next: Option<Sentence>,
    /// When the timer stops reading aloud.
    pub(super) until: Option<quireink_sdk::Instant>,
}

/// Where the underline of words `range` goes, if any is on the page.
pub(super) fn aloud_area(
    env: &Env<'_>,
    words: &[WordBox],
    range: core::ops::Range<usize>,
) -> Option<Rect> {
    let line = env.metrics.tenth_mm(5).max(2) as u16;
    selection::line_boxes(words, |i| range.contains(&i))
        .into_iter()
        .map(|b| Rect::new(b.x, b.y, b.width, b.height + 2 * line))
        .reduce(|a, b| a.union(&b))
}

impl ReaderScreen {
    /// Starts turning the pages by themselves, or stops it.
    pub(super) fn toggle_auto_turn(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.auto_turn.take().is_some() {
            cx.command(Command::Notify(
                env.i18n.tr("reader.auto-turn.stopped").to_string(),
            ));
            self.hands_free_shown(cx);
            return;
        }
        let seconds = self.auto_turn_seconds(&env);
        self.auto_turn = Some(env.now.add_millis(seconds * 1000));
        cx.command(Command::Notify(env.i18n.format(
            "reader.auto-turn.started",
            &[("seconds", Arg::Int(seconds as i64))],
        )));
        self.hands_free_shown(cx);
        self.update_status(cx);
    }

    pub(super) fn auto_turn_seconds(&self, env: &Env<'_>) -> u64 {
        env.settings
            .int(crate::settings::AUTO_TURN_SECONDS.as_str())
            .clamp(5, 600) as u64
    }

    /// Turns the page if it is time; stops at the end of the book.
    pub(super) fn auto_turn_due(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !self.auto_turn.is_some_and(|at| env.now >= at) {
            return;
        }
        let seconds = self.auto_turn_seconds(&env);
        let Ok(book) = &mut self.book else {
            return;
        };
        let end = book.page.end;
        self.auto_turn = book
            .show(cx, end, true)
            .then(|| env.now.add_millis(seconds * 1000));
        if self.auto_turn.is_none() {
            self.hands_free_shown(cx);
        }
    }

    /// Starts reading aloud on the page shown, where it was paused if
    /// that was on this page, where the device can.
    pub(super) fn start_aloud(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !env.caps.speech {
            cx.command(Command::Notify(
                env.i18n.tr("reader.aloud.unavailable").to_string(),
            ));
            return;
        }
        let Ok(book) = &self.book else {
            return;
        };
        let from = match self.resume.take() {
            Some((page, word)) if page == book.page.start => word,
            _ => 0,
        };
        let until = crate::aloud::timer_minutes(&env).map(|m| env.now.add_millis(m * 60_000));
        cx.command(Command::Notify(
            env.i18n.tr("reader.aloud.started").to_string(),
        ));
        self.begin_aloud(from, until, cx);
    }

    /// Speaks from word `from` of the page shown, handing over the next
    /// sentence at once.
    pub(super) fn begin_aloud(
        &mut self,
        from: usize,
        until: Option<quireink_sdk::Instant>,
        cx: &mut Cx<'_, '_>,
    ) {
        let env = *cx.env();
        let Ok(book) = &self.book else {
            return;
        };
        let turn = env.settings.read(&crate::settings::ALOUD_TURN);
        let (now, text) = book.sentence(&env, &book.page, from, turn);
        book.speak(&env, text, false, cx);
        let next = book.following(&env, &now, turn).map(|(next, text)| {
            book.speak(&env, text, true, cx);
            next
        });
        let before = self.aloud.map(|a| a.now);
        self.aloud = Some(Aloud { now, next, until });
        self.underline(before, Some(now), cx);
        self.hands_free_shown(cx);
    }

    /// Shows in the status line whether the book is being read aloud.
    pub(super) fn hands_free_shown(&mut self, cx: &mut Cx<'_, '_>) {
        let icon = if self.aloud.is_some() {
            Some("volume")
        } else if self.auto_turn.is_some() {
            Some("clock")
        } else {
            None
        };
        if let Ok(book) = &mut self.book
            && book.hands_free != icon
        {
            book.hands_free = icon;
            self.update_status(cx);
        }
    }

    /// Pauses reading aloud; reading aloud again goes on from there.
    pub(super) fn pause_aloud(&mut self, cx: &mut Cx<'_, '_>) {
        let Some(aloud) = self.aloud.take() else {
            return;
        };
        cx.command(Command::StopSpeaking);
        self.resume = Some((aloud.now.page, aloud.now.from));
        self.underline(Some(aloud.now), None, cx);
        self.hands_free_shown(cx);
        let env = *cx.env();
        cx.command(Command::Notify(
            env.i18n.tr("reader.aloud.paused").to_string(),
        ));
    }

    /// Moves the underline from sentence `before` to `now`, where they
    /// are on the page shown.
    pub(super) fn underline(
        &self,
        before: Option<Sentence>,
        now: Option<Sentence>,
        cx: &mut Cx<'_, '_>,
    ) {
        let env = *cx.env();
        let Ok(book) = &self.book else {
            return;
        };
        if !env.settings.read(&crate::settings::ALOUD_HIGHLIGHT) {
            return;
        }
        let words = book.words(&env);
        let area = [before, now]
            .into_iter()
            .flatten()
            .filter(|s| s.page == book.page.start)
            .filter_map(|s| aloud_area(&env, &words, s.from..s.to))
            .reduce(|a, b| a.union(&b));
        if let Some(area) = area {
            cx.request_redraw_area(area);
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    /// A sentence was read aloud: the next one is being spoken already;
    /// the one after it is handed over, turning the page where needed.
    /// Ends at the end of the book, of the page if pages are not turned,
    /// or with the timer.
    pub(super) fn spoken(&mut self, params: &Params, cx: &mut Cx<'_, '_>) {
        let Some(aloud) = self.aloud else {
            return;
        };
        let env = *cx.env();
        if params.get(quireink_sdk::speech_params::FAILED).is_some() {
            self.pause_aloud(cx);
            self.resume = None;
            cx.command(Command::Notify(
                env.i18n.tr("reader.aloud.failed").to_string(),
            ));
            return;
        }
        if aloud.until.is_some_and(|until| env.now >= until) {
            self.pause_aloud(cx);
            cx.command(Command::Notify(
                env.i18n.tr("reader.aloud.timer-done").to_string(),
            ));
            return;
        }
        let turn = env.settings.read(&crate::settings::ALOUD_TURN);
        let Some(next) = aloud.next else {
            // The end of the book, or of the page.
            self.aloud = None;
            self.underline(Some(aloud.now), None, cx);
            self.hands_free_shown(cx);
            if !turn {
                cx.command(Command::Notify(
                    env.i18n.tr("reader.aloud.page-done").to_string(),
                ));
            }
            return;
        };
        let Ok(book) = &mut self.book else {
            return;
        };
        if next.page != book.page.start {
            let end = book.page.end;
            book.show(cx, end, true);
            if book.page.start != next.page {
                // Laid out anew meanwhile: on from the page's start.
                cx.command(Command::StopSpeaking);
                return self.begin_aloud(0, aloud.until, cx);
            }
        }
        let after = book.following(&env, &next, turn).map(|(after, text)| {
            book.speak(&env, text, true, cx);
            after
        });
        self.aloud = Some(Aloud {
            now: next,
            next: after,
            until: aloud.until,
        });
        self.underline(Some(aloud.now), Some(next), cx);
    }
}

impl Book {
    /// The sentence that starts at word `from` of `page` (empty past its
    /// last word) and its text as spoken; with `turn`, one that goes on on
    /// the next page is spoken whole.
    pub(super) fn sentence(
        &self,
        env: &Env<'_>,
        page: &Page,
        from: usize,
        turn: bool,
    ) -> (Sentence, String) {
        let words = self.words_of(env, page);
        let from = from.min(words.len());
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        let to = if from < texts.len() {
            crate::aloud::sentence_end(&texts, from)
        } else {
            from
        };
        let starts_line =
            |words: &[WordBox], i: usize| i == 0 || words[i].area.y != words[i - 1].area.y;
        let mut parts: Vec<(&str, bool)> = (from..to)
            .map(|i| (texts[i], starts_line(&words, i)))
            .collect();
        // The rest of a sentence going on on the next page.
        let next_words;
        let mut carried = 0;
        let unfinished =
            to > from && to == words.len() && !crate::aloud::ends_sentence(texts[to - 1]);
        if unfinished
            && turn
            && let Ok(next) = self.layout(env, page.end)
            && !next.is_past_end()
        {
            next_words = self.words_of(env, &next);
            let next_texts: Vec<&str> = next_words.iter().map(|w| w.text.as_str()).collect();
            carried = crate::aloud::sentence_end(&next_texts, 0);
            parts.extend((0..carried).map(|i| (next_texts[i], starts_line(&next_words, i))));
        }
        let sentence = Sentence {
            page: page.start,
            from,
            to,
            words: words.len(),
            carried,
        };
        (sentence, crate::aloud::spoken(parts.into_iter()))
    }

    /// The sentence after `sentence` with its text: on its page, else
    /// (with `turn`) on the next one; `None` at the end.
    pub(super) fn following(
        &self,
        env: &Env<'_>,
        sentence: &Sentence,
        turn: bool,
    ) -> Option<(Sentence, String)> {
        let laid_out;
        let page = if sentence.page == self.page.start {
            &self.page
        } else {
            laid_out = self.layout(env, sentence.page).ok()?;
            &laid_out
        };
        if sentence.to < sentence.words {
            return Some(self.sentence(env, page, sentence.to, turn));
        }
        if !turn {
            return None;
        }
        let next = self.layout(env, page.end).ok()?;
        if next.is_past_end() || next.start == page.start {
            return None;
        }
        Some(self.sentence(env, &next, sentence.carried, turn))
    }

    /// Hands `text` over to be spoken, after what is spoken with `queue`.
    pub(super) fn speak(&self, env: &Env<'_>, text: String, queue: bool, cx: &mut Cx<'_, '_>) {
        let mut speech = quireink_sdk::Speech::new(text, crate::SPOKEN);
        speech.language = self.language.clone();
        speech.rate = env
            .settings
            .int(crate::settings::ALOUD_SPEED.as_str())
            .clamp(50, 300) as u16;
        speech.params = Params::new().with(params::PATH, self.path.clone());
        speech.queue = queue;
        cx.command(Command::Speak(speech));
    }
}
