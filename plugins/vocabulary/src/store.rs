//! The words, one a line in `words.txt`, oldest first:
//! `word<TAB>language<TAB>section<TAB>offset<TAB>book path<TAB>book title<TAB>sentence`.
//!
//! The file is only ever streamed, a line at a time, so a long list needs
//! no more memory than a page of it. It is written anew beside itself
//! (`words.tmp`) and put in its place only once complete, so a failed
//! read or write, or a power cut, never loses words.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::ControlFlow;
use core::sync::atomic::{AtomicU32, Ordering};

use paperpaw_sdk::{Env, Storage, StorageError, files};

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
/// Longest sentence kept, in characters.
const MAX_SENTENCE: usize = 300;
/// Bytes written at a time.
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

    /// The same word in the same language, whatever its case: what tells
    /// the words of the list apart.
    pub(crate) fn same(&self, word: &str, language: &str) -> bool {
        self.language == language && self.word.to_lowercase() == word.to_lowercase()
    }
}

/// Calls `line` with each line of `path`, in order (a missing file has
/// none); `Err` where the file could not be read to its end.
pub(crate) fn each_line(
    storage: &dyn Storage,
    path: &str,
    mut line: impl FnMut(&str),
) -> Result<(), StorageError> {
    let mut visit = |l: &str| {
        line(l);
        ControlFlow::Continue(())
    };
    match files::for_each_line(storage, path, &mut visit) {
        Err(StorageError::NotFound) => Ok(()),
        result => result,
    }
}

/// Calls `word` with each word of the list, oldest first; `Err` where the
/// list could not be read to its end. Lines that are no word (cut off by
/// a power cut) are skipped, and the words are numbered without them.
fn each_word(storage: &dyn Storage, mut word: impl FnMut(Word)) -> Result<(), StorageError> {
    each_line(storage, &file(), |l| {
        if let Some(w) = Word::parse(l) {
            word(w);
        }
    })
}

/// After a power cut while the words were written anew: the new list where
/// the old one is gone already (file systems that replace a file by
/// removing it first), else the unfinished one goes. Called at start.
pub(crate) fn recover(storage: &dyn Storage) {
    let (file, temporary) = (file(), temporary());
    if !storage.exists(&temporary) {
        return;
    }
    if storage.size(&file) == Err(StorageError::NotFound) {
        let _ = storage.rename(&temporary, &file);
    } else {
        let _ = storage.remove(&temporary);
    }
}

/// How many words there are.
pub(crate) fn count(storage: &dyn Storage) -> usize {
    let mut n = 0;
    let _ = each_word(storage, |_| n += 1);
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
    let _ = each_word(storage, |word| {
        summary.count += 1;
        for (seen, kind) in [(&mut books, &word.path), (&mut languages, &word.language)] {
            if !kind.is_empty() && seen.len() < MOST_KINDS && !seen.contains(kind) {
                seen.push(kind.clone());
            }
        }
        summary.newest = Some(word);
    });
    summary.books = books.len();
    summary.languages = languages.len();
    summary
}

/// The words with numbers in `from..to` (0 the oldest).
pub(crate) fn range(storage: &dyn Storage, from: usize, to: usize) -> Vec<(usize, Word)> {
    let mut words = Vec::new();
    let mut n = 0;
    let _ = each_word(storage, |word| {
        if (from..to).contains(&n) {
            words.push((n, word));
        }
        n += 1;
    });
    words
}

/// Writes the words for which `keep` holds anew, beside the list, and only
/// then puts them in its place: where anything cannot be read or written,
/// the list stays as it was.
fn rewrite(storage: &dyn Storage, mut keep: impl FnMut(&Word) -> bool) -> Result<(), StorageError> {
    let (file, temporary) = (file(), temporary());
    let mut written = || {
        storage.write(&temporary, &[])?;
        let mut out = String::new();
        let mut appended = Ok(());
        let mut visit = |l: &str| {
            if let Some(word) = Word::parse(l)
                && keep(&word)
            {
                out.push_str(&word.line());
                if out.len() >= CHUNK * 4 {
                    appended = storage.append(&temporary, out.as_bytes());
                    out.clear();
                }
            }
            if appended.is_ok() {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        };
        match files::for_each_line(storage, &file, &mut visit) {
            Ok(()) | Err(StorageError::NotFound) => {}
            Err(e) => return Err(e),
        }
        appended?;
        storage.append(&temporary, out.as_bytes())?;
        storage.rename(&temporary, &file)
    };
    let result = written();
    if result.is_err() {
        let _ = storage.remove(&temporary);
    }
    changed();
    result
}

/// Whether the list ends with a line break (or is empty): else a line cut
/// off by a power cut would run into the word appended next.
fn ends_whole(storage: &dyn Storage) -> Result<bool, StorageError> {
    let file = file();
    let size = match storage.size(&file) {
        Ok(size) => size,
        Err(StorageError::NotFound) => return Ok(true),
        Err(e) => return Err(e),
    };
    if size == 0 {
        return Ok(true);
    }
    let mut last = [0u8; 1];
    storage.read_at(&file, size - 1, &mut last)?;
    Ok(last[0] == b'\n')
}

/// Adds `word` as the newest; an older entry of it goes, and the oldest
/// words beyond [`MAX_WORDS`]. Looked up again while it is the newest, it
/// stays as it is, with the sentence it was kept with first.
pub(crate) fn add(storage: &dyn Storage, word: &Word) -> Result<(), StorageError> {
    let mut total = 0;
    let mut known = false;
    let mut newest = false;
    let read = each_word(storage, |w| {
        total += 1;
        newest = w.same(&word.word, &word.language);
        known |= newest;
    });
    // A list that cannot be read is only added to, never written anew from
    // the part of it that was read.
    if read.is_ok() {
        if newest {
            return Ok(());
        }
        if known || total >= MAX_WORDS {
            let left = total - usize::from(known);
            let drop = (left + 1).saturating_sub(MAX_WORDS);
            let mut kept = 0;
            rewrite(storage, |w| {
                if w.same(&word.word, &word.language) {
                    return false;
                }
                kept += 1;
                kept > drop
            })?;
        }
    }
    let mut line = word.line();
    // Where that cannot be told, an empty line (skipped) is safer.
    if !ends_whole(storage).unwrap_or(false) {
        line.insert(0, '\n');
    }
    storage.append(&file(), line.as_bytes())?;
    changed();
    Ok(())
}

/// Deletes `word` (the same word in the same language, [`Word::same`]),
/// wherever it is in the list by now.
pub(crate) fn remove(storage: &dyn Storage, word: &Word) -> Result<(), StorageError> {
    rewrite(storage, |w| !w.same(&word.word, &word.language))
}

/// A field of the export: in quotes where it has one (the quote doubled),
/// as flash card programs read text separated by tabs.
fn export_field(text: &str) -> Cow<'_, str> {
    if text.contains('"') {
        Cow::Owned(format!("\"{}\"", text.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(text)
    }
}

/// Writes all words to `path` for flash card programs (Anki and others):
/// a line each with the word, its sentence and the book, separated by
/// tabs, oldest first, after the lines that tell Anki how to read them.
pub(crate) fn export(storage: &dyn Storage, path: &str) -> bool {
    if storage
        .write(path, b"#separator:tab\n#html:false\n")
        .is_err()
    {
        return false;
    }
    let mut out = String::new();
    let mut ok = true;
    let read = each_word(storage, |w| {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            export_field(&w.word),
            export_field(&w.sentence),
            export_field(&w.title)
        ));
        if out.len() >= CHUNK * 4 {
            ok &= storage.append(path, out.as_bytes()).is_ok();
            out.clear();
        }
    });
    ok && read.is_ok() && storage.append(path, out.as_bytes()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    use paperpaw_sdk::{DirEntry, MemoryStorage};

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

    fn words(storage: &dyn Storage) -> Vec<String> {
        range(storage, 0, usize::MAX)
            .into_iter()
            .map(|(_, w)| w.word)
            .collect()
    }

    /// A card that fails on purpose: reads of the words from byte
    /// `read_fails_at` on, and appends to the new list after `appends`
    /// of them.
    struct Failing {
        inner: MemoryStorage,
        read_fails_at: Option<u64>,
        appends: Cell<Option<usize>>,
    }

    impl Failing {
        fn new(inner: MemoryStorage) -> Self {
            Self {
                inner,
                read_fails_at: None,
                appends: Cell::new(None),
            }
        }
    }

    impl Storage for Failing {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            if path == file() && self.read_fails_at.is_some_and(|at| offset >= at) {
                return Err(StorageError::Io);
            }
            self.inner.read_at(path, offset, buf)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.inner.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.inner.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            if path == temporary()
                && let Some(left) = self.appends.get()
            {
                if left == 0 {
                    return Err(StorageError::Full);
                }
                self.appends.set(Some(left - 1));
            }
            self.inner.append(path, data)
        }
        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.inner.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.inner.remove(path)
        }
        fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
            self.inner.rename(from, to)
        }
    }

    /// Many words, so that writing them anew takes several appends.
    fn many(n: usize) -> String {
        (0..n).map(|i| word(&format!("w{i}")).line()).collect()
    }

    #[test]
    fn adds_once_newest_last_and_removes() {
        let storage = MemoryStorage::new();
        add(&storage, &word("quire")).unwrap();
        add(&storage, &word("ink")).unwrap();
        add(&storage, &word("Quire")).unwrap();
        assert_eq!(count(&storage), 2);
        let all = range(&storage, 0, 10);
        assert_eq!(all[0].1.word, "ink");
        assert_eq!(all[1].1.word, "Quire");
        assert_eq!(all[1].1.sentence, "The Quire is here.");
        remove(&storage, &word("ink")).unwrap();
        assert_eq!(words(&storage), ["Quire"]);
        assert!(export(&storage, "out.tsv"));
        let data = paperpaw_sdk::read_file(&storage, "out.tsv", 1024).unwrap();
        assert_eq!(
            data,
            b"#separator:tab\n#html:false\nQuire\tThe Quire is here.\tA\n"
        );
    }

    #[test]
    fn the_words_at_a_glance() {
        let storage = MemoryStorage::new();
        assert_eq!(summary(&storage), Summary::default());
        add(&storage, &word("quire")).unwrap();
        add(
            &storage,
            &Word {
                language: "de".to_string(),
                path: "books/b.epub".to_string(),
                ..word("Tinte")
            },
        )
        .unwrap();
        add(&storage, &word("ink")).unwrap();
        let s = summary(&storage);
        assert_eq!((s.count, s.books, s.languages), (3, 2, 2));
        assert_eq!(s.newest.map(|w| w.word), Some("ink".to_string()));
    }

    #[test]
    fn keeps_at_most_the_newest() {
        let storage = MemoryStorage::new();
        storage.write(&file(), many(MAX_WORDS).as_bytes()).unwrap();
        add(&storage, &word("new")).unwrap();
        assert_eq!(count(&storage), MAX_WORDS);
        let first = range(&storage, 0, 1);
        assert_eq!(first[0].1.word, "w1");
        assert_eq!(range(&storage, MAX_WORDS - 1, MAX_WORDS)[0].1.word, "new");
    }

    /// Writing the list anew fails partway (the card is full): the list
    /// stays whole and the unfinished copy goes.
    #[test]
    fn a_rewrite_that_fails_partway_keeps_the_list() {
        let list = many(300);
        let storage = Failing::new(MemoryStorage::new().with(&file(), list.clone()));
        storage.appends.set(Some(1));
        assert_eq!(remove(&storage, &word("w0")), Err(StorageError::Full));
        assert_eq!(storage.inner.get(&file()), Some(list.into_bytes()));
        assert!(!storage.exists(&temporary()));
    }

    /// The list cannot be read to its end: nothing is written anew from
    /// the part that was read, and a word looked up is still kept.
    #[test]
    fn a_list_read_only_in_part_is_not_written_anew() {
        let list = many(300);
        let mut storage = Failing::new(MemoryStorage::new().with(&file(), list.clone()));
        storage.read_fails_at = Some(2048);
        assert_eq!(remove(&storage, &word("w299")), Err(StorageError::Io));
        assert_eq!(storage.inner.get(&file()), Some(list.clone().into_bytes()));
        // Looked up again: only appended, the old entry stays.
        add(&storage, &word("w0")).unwrap();
        let mut expected = list.into_bytes();
        expected.push(b'\n');
        expected.extend_from_slice(word("w0").line().as_bytes());
        assert_eq!(storage.inner.get(&file()), Some(expected));
    }

    /// A power cut between removing the old list and renaming the new one
    /// (on FAT) leaves only the new one: it is taken at start. An
    /// unfinished new one beside the old list goes.
    #[test]
    fn the_new_list_is_taken_after_a_power_cut() {
        let storage = MemoryStorage::new().with(&temporary(), word("kept").line());
        recover(&storage);
        assert_eq!(words(&storage), ["kept"]);
        assert!(!storage.exists(&temporary()));

        let storage = MemoryStorage::new()
            .with(&file(), word("old").line())
            .with(&temporary(), "half a li");
        recover(&storage);
        assert_eq!(words(&storage), ["old"]);
        assert!(!storage.exists(&temporary()));
    }

    /// A line cut off by a power cut is no word: it shifts no numbers,
    /// deleting deletes the word chosen, and the next word appended starts
    /// a line of its own.
    #[test]
    fn a_cut_line_shifts_nothing() {
        let list = format!("{}cut\ten\t1", word("first").line());
        let storage = MemoryStorage::new().with(&file(), list);
        add(&storage, &word("second")).unwrap();
        assert_eq!(count(&storage), 2);
        assert_eq!(words(&storage), ["first", "second"]);
        remove(&storage, &word("second")).unwrap();
        assert_eq!(words(&storage), ["first"]);
    }

    /// Deleting goes by the word, not by its number: the list may have
    /// changed since it was shown.
    #[test]
    fn deleting_finds_the_word_wherever_it_is() {
        let storage = MemoryStorage::new();
        for w in ["a", "b", "c"] {
            add(&storage, &word(w)).unwrap();
        }
        // "a" looked up again elsewhere: now the newest.
        add(&storage, &word("a")).unwrap();
        remove(&storage, &word("b")).unwrap();
        assert_eq!(words(&storage), ["c", "a"]);
    }

    /// The same word looked up again right after is not written anew.
    #[test]
    fn a_repeated_lookup_writes_nothing() {
        let storage = Failing::new(MemoryStorage::new());
        add(&storage, &word("quire")).unwrap();
        // Any rewrite would fail now.
        storage.appends.set(Some(0));
        let before = storage.inner.get(&file());
        assert_eq!(add(&storage, &word("Quire")), Ok(()));
        assert_eq!(storage.inner.get(&file()), before);
    }

    /// Quotes would make Anki read past the end of a field.
    #[test]
    fn the_export_quotes_quotes() {
        let storage = MemoryStorage::new();
        add(
            &storage,
            &Word {
                sentence: String::from("He said \"quire\"."),
                ..word("quire")
            },
        )
        .unwrap();
        assert!(export(&storage, "out.tsv"));
        let data = paperpaw_sdk::read_file(&storage, "out.tsv", 1024).unwrap();
        assert_eq!(
            data,
            b"#separator:tab\n#html:false\nquire\t\"He said \"\"quire\"\".\"\tA\n"
        );
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn words_of_version_1_are_read() {
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with(&super::file(), include_str!("../testdata/words.txt.v1"));
        let mut words = alloc::vec::Vec::new();
        super::each_line(&storage, &super::file(), |line| {
            words.extend(super::Word::parse(line));
        })
        .unwrap();
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
