//! The words, one a line in `words.txt`, oldest first:
//! `word<TAB>language<TAB>section<TAB>offset<TAB>book path<TAB>book title<TAB>sentence`.
//!
//! The file is only ever streamed, a line at a time, so a long list needs
//! no more memory than a page of it.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use paperpaw_sdk::{Env, Storage};

use crate::PLUGIN;

/// The changes made to the words since PaperPaw started: what the marks
/// in books depend on, known without reading the file.
static CHANGES: AtomicU32 = AtomicU32::new(0);

/// How many times the words changed since PaperPaw started.
pub(crate) fn changes() -> u32 {
    CHANGES.load(Ordering::Relaxed)
}

fn changed() {
    // Load and store: the device has no atomic adding, and one thread.
    CHANGES.store(changes().wrapping_add(1), Ordering::Relaxed);
}

/// Words kept at most; the oldest go.
pub(crate) const MAX_WORDS: usize = 2000;
/// Longest line read; longer ones are cut.
const MAX_LINE: usize = 1024;
/// Longest sentence kept, in characters.
const MAX_SENTENCE: usize = 300;
/// Bytes read or written at a time.
const CHUNK: usize = 1024;

pub(crate) fn file() -> String {
    format!("{}/words.txt", Env::plugin_dir(PLUGIN.as_str()))
}

fn temporary() -> String {
    format!("{}/words.tmp", Env::plugin_dir(PLUGIN.as_str()))
}

/// A word looked up, where and in which sentence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Word {
    pub word: String,
    pub language: String,
    pub section: u32,
    pub offset: u32,
    pub path: String,
    pub title: String,
    pub sentence: String,
}

/// Tabs and line breaks become spaces.
fn field(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

impl Word {
    pub(crate) fn line(&self) -> String {
        let sentence: String = field(&self.sentence).chars().take(MAX_SENTENCE).collect();
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            field(&self.word),
            field(&self.language),
            self.section,
            self.offset,
            field(&self.path),
            field(&self.title),
            sentence
        )
    }

    pub(crate) fn parse(line: &str) -> Option<Word> {
        let mut f = line.splitn(7, '\t');
        Some(Word {
            word: f.next()?.to_string(),
            language: f.next()?.to_string(),
            section: f.next()?.parse().ok()?,
            offset: f.next()?.parse().ok()?,
            path: f.next()?.to_string(),
            title: f.next()?.to_string(),
            sentence: f.next().unwrap_or_default().to_string(),
        })
    }

    /// The same word in the same language, whatever its case.
    fn same(&self, word: &str, language: &str) -> bool {
        self.language == language && self.word.to_lowercase() == word.to_lowercase()
    }
}

/// Calls `line` with each line of `path`, in order.
pub(crate) fn each_line(storage: &dyn Storage, path: &str, mut line: impl FnMut(&str)) {
    let mut buf = [0u8; CHUNK];
    let mut carry: Vec<u8> = Vec::new();
    let mut at = 0u64;
    loop {
        let n = storage.read_at(path, at, &mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        at += n as u64;
        for &b in &buf[..n] {
            if b == b'\n' {
                line(&String::from_utf8_lossy(&carry));
                carry.clear();
            } else if carry.len() < MAX_LINE {
                carry.push(b);
            }
        }
    }
    if !carry.is_empty() {
        line(&String::from_utf8_lossy(&carry));
    }
}

/// How many words there are.
pub(crate) fn count(storage: &dyn Storage) -> usize {
    let mut n = 0;
    each_line(storage, &file(), |l| n += usize::from(!l.is_empty()));
    n
}

/// Books and languages told apart at most in a [`Summary`].
const MOST_KINDS: usize = 32;

/// The words at a glance: how many, the newest, from how many books and
/// in how many languages (up to [`MOST_KINDS`] of each).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub count: usize,
    pub newest: Option<Word>,
    pub books: usize,
    pub languages: usize,
}

/// The words at a glance, read in one pass.
pub(crate) fn summary(storage: &dyn Storage) -> Summary {
    let mut summary = Summary::default();
    let mut books: Vec<String> = Vec::new();
    let mut languages: Vec<String> = Vec::new();
    let mut newest = String::new();
    each_line(storage, &file(), |l| {
        let Some(word) = Word::parse(l) else {
            return;
        };
        summary.count += 1;
        for (seen, kind) in [(&mut books, &word.path), (&mut languages, &word.language)] {
            if !kind.is_empty() && seen.len() < MOST_KINDS && !seen.contains(kind) {
                seen.push(kind.clone());
            }
        }
        newest.clear();
        newest.push_str(l);
    });
    summary.newest = Word::parse(&newest);
    summary.books = books.len();
    summary.languages = languages.len();
    summary
}

/// The words with numbers in `from..to` (0 the oldest).
pub(crate) fn range(storage: &dyn Storage, from: usize, to: usize) -> Vec<(usize, Word)> {
    let mut words = Vec::new();
    let mut n = 0;
    each_line(storage, &file(), |l| {
        if l.is_empty() {
            return;
        }
        if (from..to).contains(&n)
            && let Some(word) = Word::parse(l)
        {
            words.push((n, word));
        }
        n += 1;
    });
    words
}

/// Writes the words for which `keep` holds (with their numbers) anew.
fn rewrite(storage: &dyn Storage, mut keep: impl FnMut(usize, &Word) -> bool) {
    let (file, temporary) = (file(), temporary());
    if storage.write(&temporary, &[]).is_err() {
        return;
    }
    let mut out = String::new();
    let mut n = 0;
    each_line(storage, &file, |l| {
        let Some(word) = Word::parse(l) else {
            return;
        };
        if keep(n, &word) {
            out.push_str(&word.line());
            if out.len() >= CHUNK * 4 {
                let _ = storage.append(&temporary, out.as_bytes());
                out.clear();
            }
        }
        n += 1;
    });
    let _ = storage.append(&temporary, out.as_bytes());
    let _ = storage.rename(&temporary, &file);
    changed();
}

/// Adds `word` as the newest; an older entry of it goes, and the oldest
/// words beyond [`MAX_WORDS`].
pub(crate) fn add(storage: &dyn Storage, word: &Word) {
    let mut total = 0;
    let mut known = false;
    each_line(storage, &file(), |l| {
        if let Some(w) = Word::parse(l) {
            total += 1;
            known |= w.same(&word.word, &word.language);
        }
    });
    if known || total >= MAX_WORDS {
        let left = total - usize::from(known);
        let drop = (left + 1).saturating_sub(MAX_WORDS);
        let mut kept = 0;
        rewrite(storage, |_, w| {
            if w.same(&word.word, &word.language) {
                return false;
            }
            kept += 1;
            kept > drop
        });
    }
    let _ = storage.append(&file(), word.line().as_bytes());
    changed();
}

/// Deletes word number `n`.
pub(crate) fn remove(storage: &dyn Storage, n: usize) {
    rewrite(storage, |i, _| i != n);
}

/// Writes all words to `path` for flash card programs (Anki and others):
/// a line each with the word, its sentence and the book, separated by
/// tabs, oldest first.
pub(crate) fn export(storage: &dyn Storage, path: &str) -> bool {
    if storage.write(path, &[]).is_err() {
        return false;
    }
    let mut out = String::new();
    let mut ok = true;
    each_line(storage, &file(), |l| {
        let Some(w) = Word::parse(l) else {
            return;
        };
        out.push_str(&format!("{}\t{}\t{}\n", w.word, w.sentence, w.title));
        if out.len() >= CHUNK * 4 {
            ok &= storage.append(path, out.as_bytes()).is_ok();
            out.clear();
        }
    });
    ok && storage.append(path, out.as_bytes()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    fn word(word: &str) -> Word {
        Word {
            word: word.to_string(),
            language: "en".to_string(),
            section: 2,
            offset: 40,
            path: "books/a.epub".to_string(),
            title: "A".to_string(),
            sentence: format!("The {word}\tis here."),
        }
    }

    #[test]
    fn adds_once_newest_last_and_removes() {
        let storage = MemoryStorage::new();
        add(&storage, &word("quire"));
        add(&storage, &word("ink"));
        add(&storage, &word("Quire"));
        assert_eq!(count(&storage), 2);
        let all = range(&storage, 0, 10);
        assert_eq!(all[0].1.word, "ink");
        assert_eq!(all[1].1.word, "Quire");
        assert_eq!(all[1].1.sentence, "The Quire is here.");
        remove(&storage, 0);
        assert_eq!(range(&storage, 0, 10)[0].1.word, "Quire");
        assert!(export(&storage, "out.tsv"));
        let data = paperpaw_sdk::read_file(&storage, "out.tsv", 1024).unwrap();
        assert_eq!(data, b"Quire\tThe Quire is here.\tA\n");
    }

    #[test]
    fn the_words_at_a_glance() {
        let storage = MemoryStorage::new();
        assert_eq!(summary(&storage), Summary::default());
        add(&storage, &word("quire"));
        add(
            &storage,
            &Word {
                language: "de".to_string(),
                path: "books/b.epub".to_string(),
                ..word("Tinte")
            },
        );
        add(&storage, &word("ink"));
        let s = summary(&storage);
        assert_eq!((s.count, s.books, s.languages), (3, 2, 2));
        assert_eq!(s.newest.map(|w| w.word), Some("ink".to_string()));
    }

    #[test]
    fn keeps_at_most_the_newest() {
        let storage = MemoryStorage::new();
        let many: String = (0..MAX_WORDS)
            .map(|n| word(&format!("w{n}")).line())
            .collect();
        storage.write(&file(), many.as_bytes()).unwrap();
        add(&storage, &word("new"));
        assert_eq!(count(&storage), MAX_WORDS);
        let first = range(&storage, 0, 1);
        assert_eq!(first[0].1.word, "w1");
        assert_eq!(range(&storage, MAX_WORDS - 1, MAX_WORDS)[0].1.word, "new");
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn words_of_version_1_are_read() {
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with(&super::file(), include_str!("../testdata/words.txt.v1"));
        let mut words = alloc::vec::Vec::new();
        super::each_line(&storage, &super::file(), |line| {
            words.extend(super::Word::parse(line));
        });
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "Gregor");
        assert_eq!(words[0].language, "de");
        assert_eq!((words[0].section, words[0].offset), (2, 1234));
        assert_eq!(words[0].title, "Die Verwandlung");
        assert_eq!(
            words[0].sentence,
            "Als Gregor Samsa eines Morgens erwachte."
        );
        assert_eq!(words[1].path, "books/alice.txt");
        assert_eq!(words[1].sentence, "");
    }
}
