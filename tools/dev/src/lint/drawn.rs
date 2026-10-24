//! What screens and widgets draw in the pseudo-language, against what
//! they draw in English: text that fits in English but is cut or
//! shortened once longer, text shown unaccented (not from the catalogs),
//! keys shown as they are. The thorough checks of what is drawn are the
//! visual module's; these are simple ones on the described parts.

use paperpaw_core::sdk::semantics::Described;
use paperpaw_graphics::Role;
use paperpaw_platform::Rect;

use super::pseudo;
use crate::report::Finding;

/// How text is measured: the UI font at its smallest size (the notes'),
/// so that what does not fit so certainly does not fit as drawn.
pub trait Measure {
    fn width(&self, text: &str) -> f32;
    fn line_height(&self) -> f32;
    /// How many lines `text` takes when wrapped to `width`.
    fn lines(&self, text: &str, width: f32) -> usize;
}

/// Whether `text` is (partly) in the pseudo-language.
pub fn is_pseudo(text: &str) -> bool {
    text.chars().any(pseudo::is_accented) || (text.contains('[') && text.contains('~'))
}

/// Whether described text does not fit its room as measured: it takes
/// more lines than the room holds, or one line wider than it.
fn overflows(d: &Described, measure: &dyn Measure) -> bool {
    let Some(room) = d.area.intersection(&d.clip) else {
        return false;
    };
    if room.width == 0 || room.height == 0 {
        return false;
    }
    let line = measure.line_height();
    let label = d.label.trim();
    // A pixel or two less than a line still holds it.
    let holds = ((f32::from(room.height) + 2.0) / line).floor().max(1.0) as usize;
    let width = f32::from(room.width);
    let lines = measure.lines(label, width);
    lines > holds || (holds == 1 && measure.width(label) > width + 1.0)
}

/// The English part drawn as `d` was: the same role in the same place,
/// the first not taken yet.
fn counterpart<'a>(
    d: &Described,
    english: &'a [Described],
    taken: &mut [bool],
) -> Option<&'a Described> {
    let i = english
        .iter()
        .enumerate()
        .position(|(i, e)| !taken[i] && e.role == d.role && e.area == d.area)?;
    taken[i] = true;
    Some(&english[i])
}

/// A short form of a long text, for messages.
fn short(text: &str) -> String {
    let text = text.replace('\n', " ");
    if text.chars().count() > 60 {
        let cut: String = text.chars().take(57).collect();
        format!("{cut}...")
    } else {
        text
    }
}

/// How many of a place's texts a message lists.
const LISTED: usize = 3;

/// `items` for a message: the first few, and how many more.
fn listed(items: &[String]) -> String {
    let mut text = items
        .iter()
        .take(LISTED)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if items.len() > LISTED {
        text.push_str(&format!("; and {} more", items.len() - LISTED));
    }
    text
}

/// Text drawn in the pseudo-language that does not fit its room though
/// its English does: one finding for the place, with the first one's area.
pub fn overflow(
    plugin: &str,
    place: &str,
    english: &[Described],
    drawn: &[Described],
    measure: &dyn Measure,
) -> Option<(Finding, Rect)> {
    let mut taken = vec![false; english.len()];
    let mut texts = Vec::new();
    let mut first = None;
    for d in drawn {
        if d.role != Role::Text || !is_pseudo(&d.label) {
            continue;
        }
        let before = counterpart(d, english, &mut taken);
        if !overflows(d, measure) || before.is_some_and(|e| overflows(e, measure)) {
            continue;
        }
        first.get_or_insert(d.area);
        texts.push(format!(
            "`{}`{}",
            short(&d.label),
            before.map_or(String::new(), |e| format!(
                " (English `{}`)",
                short(&e.label)
            ))
        ));
    }
    let area = first?;
    let finding = Finding::warning(
        "lint.pseudo-overflow",
        place,
        format!(
            "{} a third longer than English no longer {} and {} cut or shortened: {}; \
             leave room for longer translations (wrap, or take more lines).",
            if texts.len() == 1 {
                String::from("A text")
            } else {
                format!("{} texts", texts.len())
            },
            if texts.len() == 1 { "fits" } else { "fit" },
            if texts.len() == 1 { "is" } else { "are" },
            listed(&texts)
        ),
    )
    .plugin(plugin.to_string());
    Some((finding, area))
}

/// Words that are data in sessions, not texts of the app: the sample
/// books and what is said of them (the library's file names are added).
const DATA: &[&str] = &[
    "alice",
    "adventures",
    "wonderland",
    "lewis",
    "carroll",
    "die",
    "verwandlung",
    "franz",
    "kafka",
];

/// The parts of `text` outside the pseudo-language's brackets.
fn outside_brackets(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => {
                depth -= 1;
                out.push(' ');
            }
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// What is known of a session besides what it drew, to tell text the
/// app forgot to translate from data and ids.
pub struct Known<'a> {
    /// The plugins of the app, whose keys are looked for.
    pub plugins: &'a [String],
    /// Whether a word is an id the app has (a screen, a setting, a
    /// widget, an action): those are shown as such at times (to tell
    /// screens of the same name apart, say).
    pub id: &'a dyn Fn(&str) -> bool,
    /// Words of the library (file and folder names), lowercase.
    pub data: &'a [String],
    /// What the same screens drew in another language than English, if
    /// the app has one: text the same there is no content of a language
    /// (as quotes are), shown in English for want of the pseudo-language.
    pub elsewhere: Option<&'a [String]>,
}

/// Words of plain English in `text` outside the pseudo-language: letters
/// only, three or more, not all capitals (abbreviations, units), not
/// file names, addresses or data of the session. Names, every word
/// capitalised (a book's title, a catalog's), are data too.
fn plain_words(text: &str, data: &[String]) -> Vec<String> {
    let words: Vec<String> = outside_brackets(text)
        .split_whitespace()
        // File names, addresses, times.
        .filter(|token| !token.contains(['.', '/', ':', '@', '\\']) || token.ends_with(['.', ':']))
        .flat_map(|token| {
            token
                .split(|c: char| !c.is_alphanumeric())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|w| {
            let lower = w.to_ascii_lowercase();
            w.chars().count() >= 3
                && w.chars().all(|c| c.is_ascii_alphabetic())
                && w.chars().any(|c| c.is_ascii_lowercase())
                && !DATA.contains(&lower.as_str())
                && !data.contains(&lower)
        })
        .collect();
    let name = words
        .iter()
        .all(|w| w.starts_with(|c: char| c.is_ascii_uppercase()));
    if name { Vec::new() } else { words }
}

/// Whether `word` is a translation key of one of the app's plugins shown
/// as it is: `<plugin>.<more>`, and no id the app has.
fn shown_key(word: &str, known: &Known<'_>) -> bool {
    super::catalogs::well_formed(word)
        && known.plugins.iter().any(|p| p == super::namespace(word))
        && !(known.id)(word)
}

/// Texts drawn unaccented (hard-coded English, or text from elsewhere
/// than the catalogs) and keys shown as they are, in what `plugin` drew
/// at `place` in the pseudo-language: a finding for each kind.
pub fn untranslated(
    plugin: &str,
    place: &str,
    drawn: &[Described],
    known: &Known<'_>,
) -> Vec<Finding> {
    let mut keys: Vec<String> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for d in drawn {
        let label = d.label.trim();
        if label.is_empty() {
            continue;
        }
        // Keys shown are said as such, not as plain words.
        let mut rest = outside_brackets(label);
        let words: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
        for word in words {
            let word = word.trim_matches(|c: char| !c.is_alphanumeric());
            if shown_key(word, known) {
                rest = rest.replace(word, " ");
                if !keys.iter().any(|k| k == word) {
                    keys.push(word.to_string());
                }
            }
        }
        let content = known
            .elsewhere
            .is_some_and(|other| !other.iter().any(|o| o.trim() == label));
        if !content
            && !plain_words(&rest, known.data).is_empty()
            && !texts.iter().any(|t| *t == format!("`{}`", short(label)))
        {
            texts.push(format!("`{}`", short(label)));
        }
    }
    let mut findings = Vec::new();
    if !keys.is_empty() {
        let keys: Vec<String> = keys.iter().map(|k| format!("`{k}`")).collect();
        findings.push(
            Finding::warning(
                "lint.missing-key",
                place,
                format!(
                    "Shows {} as {}: no catalog has {}; add {} to the plugin's \
                     `i18n/en.ini` (and the other languages).",
                    listed(&keys),
                    if keys.len() == 1 { "it is" } else { "they are" },
                    if keys.len() == 1 {
                        "the key"
                    } else {
                        "the keys"
                    },
                    if keys.len() == 1 { "it" } else { "them" },
                ),
            )
            .plugin(plugin.to_string()),
        );
    }
    if !texts.is_empty() {
        findings.push(
            Finding::warning(
                "lint.hardcoded",
                place,
                format!(
                    "Shows {} untranslated in the pseudo-language: text from the program \
                     rather than the catalogs; give it a key and translate it with \
                     `env.i18n().tr`.",
                    listed(&texts)
                ),
            )
            .plugin(plugin.to_string()),
        );
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character as wide as `width`, lines `line` high.
    struct Fixed {
        width: f32,
        line: f32,
    }

    impl Measure for Fixed {
        fn width(&self, text: &str) -> f32 {
            text.chars().count() as f32 * self.width
        }
        fn line_height(&self) -> f32 {
            self.line
        }
        fn lines(&self, text: &str, width: f32) -> usize {
            let per_line = (width / self.width).max(1.0) as usize;
            text.chars().count().div_ceil(per_line).max(1)
        }
    }

    fn text(label: &str, area: Rect, clip: Rect) -> Described {
        Described::new(Role::Text, label.to_string(), area).clip(clip)
    }

    const SCREEN: Rect = Rect::new(0, 0, 480, 800);

    #[test]
    fn longer_text_that_no_longer_fits() {
        let measure = Fixed {
            width: 10.0,
            line: 20.0,
        };
        let room = Rect::new(0, 0, 100, 20);
        let english = [
            text("Settings", room, SCREEN),
            text("Far too long here", room, SCREEN),
        ];
        let drawn = [
            text(&pseudo::pseudo("Settings"), room, SCREEN),
            text(&pseudo::pseudo("Far too long here"), room, SCREEN),
            text(&pseudo::pseudo("Ok"), room, SCREEN),
        ];
        let (finding, area) = overflow("demo", "demo.screen", &english, &drawn, &measure).unwrap();
        // The second is cut in English already: the visual checks say so.
        assert!(finding.message.starts_with("A text"), "{}", finding.message);
        assert!(finding.message.contains("English `Settings`"));
        assert_eq!(area, room);

        // Text wrapped over lines, within a cell that clips it.
        let cell = Rect::new(0, 0, 100, 50);
        let block = Rect::new(0, 0, 100, 100);
        let english = [text("Two short lines.", block, cell)];
        let drawn = [text(
            &pseudo::pseudo("Two short lines of text."),
            block,
            cell,
        )];
        assert!(overflow("demo", "w", &english, &drawn, &measure).is_some());
        let drawn = [text(&pseudo::pseudo("Two."), block, cell)];
        assert!(overflow("demo", "w", &english, &drawn, &measure).is_none());
    }

    #[test]
    fn unaccented_text_and_keys() {
        let plugins = vec![String::from("demo"), String::from("core")];
        let id = |id: &str| id == "demo.choice";
        let data = vec![String::from("plugins")];
        let elsewhere = vec![
            String::from("Loading books"),
            String::from("demo.setting.face"),
        ];
        let known = Known {
            plugins: &plugins,
            id: &id,
            data: &data,
            elsewhere: Some(&elsewhere),
        };
        let at = Rect::new(0, 0, 100, 20);
        let drawn = [
            text(&pseudo::pseudo("Settings"), at, SCREEN),
            text("Loading books", at, SCREEN),
            text("demo.setting.face", at, SCREEN),
            text(
                &format!("{} (demo.choice)", pseudo::pseudo("Choose")),
                at,
                SCREEN,
            ),
            text(&format!("{} 12 %", pseudo::pseudo("Battery")), at, SCREEN),
            text("Alice's Adventures in Wonderland", at, SCREEN),
            text("Project Gutenberg", at, SCREEN),
            text("USB OK 3 MB", at, SCREEN),
            text("plugins", at, SCREEN),
            text("paperpaw.ini", at, SCREEN),
            text("www.gutenberg.org", at, SCREEN),
            // A quote: content of a language, other in German.
            text("Some books are to be tasted", at, SCREEN),
            text(
                &format!("{}: Alice", pseudo::pseudo("Reading {title}")),
                at,
                SCREEN,
            ),
        ];
        let found = untranslated("demo", "demo.screen", &drawn, &known);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].check, "lint.missing-key");
        assert!(
            found[0]
                .message
                .starts_with("Shows `demo.setting.face` as it is")
        );
        assert_eq!(found[1].check, "lint.hardcoded");
        assert!(
            found[1]
                .message
                .starts_with("Shows `Loading books` untranslated")
        );
    }
}
