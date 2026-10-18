//! Marks on the page's words, all drawn the same way (ADR 0021): the
//! search hit gone to, the sentence read aloud, one's own highlights,
//! what plugins' decorators mark, links. Each is a span of the page in a
//! role's look; where spans meet on a word their looks are combined, the
//! earlier source winning an aspect both set.

use quireink_sdk::builtin::roles;
use quireink_sdk::decorations::{self, Decoration, PageText, PageWord};

use super::*;

/// A part of the page, in a look.
struct Span {
    start: Locator,
    end: Locator,
    look: Decoration,
}

/// The marks of a page, ready to draw: the boxes of words that look the
/// same (one per line), and each word's ink.
pub(super) struct PageMarks {
    groups: Vec<(Decoration, Vec<Rect>)>,
    pub(super) inks: Vec<Option<Color>>,
}

impl PageMarks {
    /// Backgrounds, before the text.
    pub(super) fn draw_behind(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        for (look, boxes) in &self.groups {
            decorations::draw_behind(canvas, env, boxes, look);
        }
    }

    /// Lines, frames and strikes, after the text.
    pub(super) fn draw_over(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        for (look, boxes) in &self.groups {
            decorations::draw_over(canvas, env, boxes, look);
        }
    }
}

impl Book {
    /// Asks the decorators for this page's marks if the page or what their
    /// marks depend on changed since they were asked.
    pub(super) fn update_decorations(&mut self, env: &Env<'_>, words: &[WordBox]) {
        if env.decorators.is_empty() {
            self.decorated.clear();
            return;
        }
        let key = (self.page.start, env.decorators.signature(env));
        if self.decorated_for == Some(key) {
            return;
        }
        let page_words: Vec<PageWord<'_>> = words
            .iter()
            .map(|w| PageWord {
                text: &w.text,
                start: w.start,
                end: w.end,
            })
            .collect();
        let page = PageText {
            path: &self.path,
            language: &self.language,
            words: &page_words,
        };
        self.decorated = env.decorators.decorate(env, &page);
        self.decorated_for = Some(key);
    }

    /// The decorator's mark that takes taps at word `index`, if any.
    pub(super) fn tappable_at(&self, words: &[WordBox], index: usize) -> Option<usize> {
        let word = words.get(index)?;
        self.decorated
            .iter()
            .position(|m| m.taps && m.overlaps(word.start, word.end))
    }

    /// Tells the decorator of mark `index` that it was tapped (or chosen
    /// with the buttons).
    pub(super) fn tap_mark(&self, words: &[WordBox], index: usize, cx: &mut Cx<'_, '_>) {
        let Some(mark) = self.decorated.get(index) else {
            return;
        };
        let text: Vec<&str> = words
            .iter()
            .filter(|w| mark.overlaps(w.start, w.end))
            .map(|w| w.text.as_str())
            .collect();
        cx.command(events::DECORATION_TAPPED.emit(events::DecorationTapped {
            decorator: mark.decorator.clone(),
            role: mark.role.clone(),
            text: text.join(" "),
            path: self.path.clone(),
            section: mark.start.section,
            offset: mark.start.offset,
            end_section: mark.end.section,
            end_offset: mark.end.offset,
        }));
    }

    /// Every mark of the page, by priority, as spans in their looks.
    fn spans(&self, env: &Env<'_>, aloud: Option<(usize, usize)>, words: &[WordBox]) -> Vec<Span> {
        let mut looks: Vec<(Id, Decoration)> = Vec::new();
        let mut look = |role: &Id| match looks.iter().find(|(r, _)| r == role) {
            Some((_, look)) => *look,
            None => {
                let found = Decoration::of_role(env, role);
                looks.push((role.clone(), found));
                found
            }
        };
        let mut spans = Vec::new();
        if let Some((start, end)) = self.found {
            spans.push(Span {
                start,
                end,
                look: look(&roles::SEARCH_HIT),
            });
        }
        if let Some((from, to)) = aloud
            && let (Some(first), Some(last)) = (words.get(from), words.get(to.saturating_sub(1)))
            && from < to
        {
            spans.push(Span {
                start: first.start,
                end: last.end,
                look: look(&roles::ALOUD),
            });
        }
        let (page_start, page_end) = (self.page.start, self.page.end);
        for mark in self
            .marks
            .iter()
            .filter(|m| m.kind == Kind::Highlight && m.overlaps(page_start, page_end))
        {
            spans.push(Span {
                start: mark.start,
                end: mark.end,
                look: look(&roles::HIGHLIGHT),
            });
        }
        for mark in &self.decorated {
            spans.push(Span {
                start: mark.start,
                end: mark.end,
                look: look(&mark.role),
            });
        }
        let web = env.has_slot(&slots::BROWSER);
        let section = self.page.start.section;
        for link in self
            .links
            .iter()
            .filter(|l| web || !quireink_sdk::document::is_external(&l.href))
        {
            spans.push(Span {
                start: Locator::new(section, link.start),
                end: Locator::new(section, link.end),
                look: look(&roles::LINK),
            });
        }
        spans
    }

    /// The marks of the page as they are drawn: words that look the same
    /// next to each other in one box per line, and each word's ink.
    /// `aloud`: the words read aloud now, if on this page.
    pub(super) fn page_marks(
        &self,
        env: &Env<'_>,
        aloud: Option<(usize, usize)>,
        words: &[WordBox],
    ) -> PageMarks {
        let spans = self.spans(env, aloud, words);
        let mut inks = Vec::with_capacity(words.len());
        let mut groups: Vec<(Decoration, Vec<Rect>)> = Vec::new();
        let mut run: Option<(Decoration, Vec<(usize, Rect)>)> = None;
        for (index, word) in words.iter().enumerate() {
            let look = decorations::combine(
                spans
                    .iter()
                    .filter(|s| s.start < word.end && word.start < s.end)
                    .map(|s| s.look),
            );
            inks.push(look.ink_color());
            match &mut run {
                Some((current, members)) if *current == look => members.push((index, word.area)),
                _ => {
                    if let Some((done, members)) = run.take()
                        && !done.is_none()
                    {
                        groups.push((done, decorations::join_boxes(members)));
                    }
                    run = Some((look, Vec::from([(index, word.area)])));
                }
            }
        }
        if let Some((done, members)) = run
            && !done.is_none()
        {
            groups.push((done, decorations::join_boxes(members)));
        }
        PageMarks { groups, inks }
    }
}
