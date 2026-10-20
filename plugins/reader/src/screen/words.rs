//! The words of the page, with their boxes on the screen and places in
//! the book: what is chosen, looked up and read aloud.

use super::*;

/// A word on the page, with its box on the screen and its place in the
/// book.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WordBox {
    pub area: Rect,
    pub text: String,
    pub start: Locator,
    pub end: Locator,
}

impl WordBox {
    /// The word without surrounding punctuation, as looked up.
    pub(super) fn lookup_text(&self) -> &str {
        self.text.trim_matches(|c: char| !c.is_alphanumeric())
    }
}

/// Most words of a sentence on either side of a looked-up word.
pub(super) const SENTENCE_WORDS: usize = 30;

/// The sentence around words `lo..=hi` of a page, as far as the page has
/// it.
pub(super) fn sentence(words: &[WordBox], lo: usize, hi: usize) -> String {
    let ends = |w: &WordBox| {
        w.text
            .trim_end_matches(['"', '\'', '’', '”', '»', '«', ')'])
            .ends_with(['.', '!', '?', '…'])
    };
    let mut first = lo;
    while first > 0 && lo - first < SENTENCE_WORDS && !ends(&words[first - 1]) {
        first -= 1;
    }
    let mut last = hi;
    while last + 1 < words.len() && last - hi < SENTENCE_WORDS && !ends(&words[last]) {
        last += 1;
    }
    words[first..=last]
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

impl Book {
    /// The words of the page with their boxes on the screen, in reading
    /// order, measured as they are drawn: once a page (a page drawn in
    /// bands is drawn several times).
    pub(super) fn words(&self, env: &Env<'_>) -> &[WordBox] {
        self.words.get_or_init(|| self.words_of(env, &self.page))
    }

    /// The words of `page`, as [`Book::words`].
    pub(super) fn words_of(&self, env: &Env<'_>, page: &Page) -> Vec<WordBox> {
        let t = &self.typography;
        let Some(faces) = Faces::new(env, t, self.embedded.as_ref()) else {
            return Vec::new();
        };
        let mut words: Vec<WordBox> = Vec::new();
        for line in &page.lines {
            let mut x = t.x + line.x;
            // Inside a word that continues in the next run (e.g. bold).
            let mut joined = false;
            // The next of the line's words.
            let mut ordinal = 0;
            for run in &line.runs {
                let (ascent, height) = faces.line(run.face);
                let top = t.y + line.baseline - ascent;
                let space = faces.space(run.face) + line.word_spacing;
                for (i, part) in line.text[run.start..run.end].split(' ').enumerate() {
                    if i > 0 {
                        x += space;
                        joined = false;
                    }
                    if part.is_empty() {
                        continue;
                    }
                    let width = faces.measure(run.face, part);
                    match words.last_mut() {
                        Some(last) if joined => {
                            last.text.push_str(part);
                            last.area.width = (x + width) as u16 - last.area.x;
                        }
                        _ => {
                            let (start, end) = line.words.get(ordinal).copied().unwrap_or_default();
                            ordinal += 1;
                            words.push(WordBox {
                                area: Rect::new(x as u16, top as u16, width as u16, height as u16),
                                text: part.to_string(),
                                start: Locator::new(line.section, start),
                                end: Locator::new(line.section, end),
                            });
                        }
                    }
                    x += width;
                    joined = true;
                }
            }
        }
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<WordBox> {
        text.split(' ')
            .map(|word| WordBox {
                area: Rect::new(0, 0, 1, 1),
                text: String::from(word),
                start: Locator::new(0, 0),
                end: Locator::new(0, 0),
            })
            .collect()
    }

    #[test]
    fn the_sentence_around_a_word() {
        let page = words("It was late. \"Oh dear!\" said the Rabbit. Alice ran.");
        // "Rabbit." is word 7: its sentence begins after "dear!\"".
        assert_eq!(sentence(&page, 7, 7), "said the Rabbit.");
        assert_eq!(sentence(&page, 0, 0), "It was late.");
        assert_eq!(sentence(&page, 9, 9), "Alice ran.");
        // Several words: from the first one's sentence to the last one's.
        assert_eq!(sentence(&page, 2, 3), "It was late. \"Oh dear!\"");
    }

    #[test]
    fn words_are_looked_up_without_punctuation() {
        let page = words("\"Rabbit,\" (dear) well-known");
        let looked: Vec<&str> = page.iter().map(WordBox::lookup_text).collect();
        assert_eq!(looked, ["Rabbit", "dear", "well-known"]);
    }
}
