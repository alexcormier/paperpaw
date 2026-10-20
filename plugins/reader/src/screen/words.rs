//! The words of the page, with their boxes on the screen and places in
//! the book: what is chosen, looked up and read aloud.

use super::*;

pub(crate) use paperpaw_sdk::reading::WordBox;

/// Most words of a sentence on either side of a looked-up word.
pub(super) const SENTENCE_WORDS: usize = 30;

/// The sentence around words `lo..=hi` of a page, as far as the page has
/// it.
pub(super) fn sentence(words: &[WordBox], lo: usize, hi: usize) -> String {
    let ends = |w: &WordBox| crate::aloud::ends_sentence(&w.text);
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
        Faces::new(env, t, self.embedded.as_ref()).map_or_else(Vec::new, |faces| {
            paperpaw_sdk::reading::word_boxes(&faces, t, page)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<WordBox> {
        text.split(' ')
            .map(|word| WordBox::new(Rect::new(0, 0, 1, 1), word, Locator::START, Locator::START))
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
}
