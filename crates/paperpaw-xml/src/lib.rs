//! A streaming, forgiving XML/XHTML tokenizer.
//!
//! Data is fed in chunks (e.g. straight from a ZIP inflater); complete
//! tokens are emitted immediately and incomplete ones are kept until the
//! next chunk, so memory use is bounded by the largest token, not the
//! document. It is forgiving like e-readers must be: malformed markup never
//! aborts, it degrades to text or is skipped.
//!
//! There is no DOM and no namespace processing; names keep their prefix
//! (see [`local_name`]).
//!
//! For HTML ([`Tokenizer::html`]), the content of `<script>` and `<style>`
//! is text up to the matching end tag, as browsers read it: `a < b` in a
//! script starts no tag.

#![no_std]

extern crate alloc;

mod entities;

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

pub use entities::decode;

/// Writes `text` onto `out` safe as XML or HTML text and attribute values:
/// `&`, `<`, `>` and `"` as entities.
pub fn escape(text: &str, out: &mut alloc::string::String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// Unfinished text runs longer than this are emitted up to their last
/// whitespace, so long paragraphs never pile up in the buffer.
const EAGER_TEXT: usize = 256;
/// Unfinished text runs without whitespace are cut at this length.
const MAX_TEXT: usize = 1024;
/// Markup longer than this (e.g. an inline base64 image) is skipped.
const MAX_MARKUP: usize = 4 * 1024;
/// Input is tokenized in pieces of this size.
const PIECE: usize = 1024;

/// A token. Text is raw: decode entities with [`decode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token<'a> {
    Start {
        name: &'a str,
        attrs: Attrs<'a>,
        /// `<br/>`-style tag without end tag.
        self_closing: bool,
    },
    End {
        name: &'a str,
    },
    Text(Cow<'a, str>),
    /// Content of a CDATA section (no entity decoding needed).
    CData(&'a str),
}

/// The raw attribute part of a start tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attrs<'a>(&'a str);

impl<'a> Attrs<'a> {
    /// The decoded value of attribute `name` (exact name, prefix included).
    pub fn get(&self, name: &str) -> Option<Cow<'a, str>> {
        self.iter()
            .find(|(n, _)| *n == name)
            .map(|(_, value)| decode(value))
    }

    /// `(name, raw value)` pairs; malformed attributes are skipped.
    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        AttrIter(self.0)
    }
}

struct AttrIter<'a>(&'a str);

impl<'a> Iterator for AttrIter<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let rest = self.0.trim_start();
            if rest.is_empty() {
                return None;
            }
            let name_end = rest
                .find(|c: char| c == '=' || c.is_whitespace())
                .unwrap_or(rest.len());
            let name = &rest[..name_end];
            let after = rest[name_end..].trim_start();
            let Some(after_eq) = after.strip_prefix('=') else {
                // Attribute without value (HTML style): skip it.
                self.0 = after;
                if name.is_empty() {
                    self.0 = &rest[1..];
                }
                continue;
            };
            let value_part = after_eq.trim_start();
            let (value, remaining) = match value_part.chars().next() {
                Some(q @ ('"' | '\'')) => match value_part[1..].find(q) {
                    Some(end) => (&value_part[1..1 + end], &value_part[2 + end..]),
                    None => (&value_part[1..], ""),
                },
                _ => {
                    let end = value_part
                        .find(char::is_whitespace)
                        .unwrap_or(value_part.len());
                    (&value_part[..end], &value_part[end..])
                }
            };
            self.0 = remaining;
            if !name.is_empty() {
                return Some((name, value));
            }
        }
    }
}

/// The part of a qualified name after its prefix: `dc:title` → `title`.
pub fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Elements whose content HTML reads as text up to their end tag.
const RAW_TEXT: [&str; 2] = ["script", "style"];

/// Incremental tokenizer; see the crate documentation.
#[derive(Debug, Default)]
pub struct Tokenizer {
    pending: Vec<u8>,
    /// Skipping overlong markup up to its end.
    skip: Option<Skip>,
    /// HTML: `<script>` and `<style>` hold raw text.
    html: bool,
    /// Inside such an element: its name.
    raw: Option<&'static str>,
}

impl Tokenizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// A tokenizer for HTML (see the crate documentation).
    pub fn html() -> Self {
        Self {
            html: true,
            ..Self::default()
        }
    }

    /// Tokenizes a complete document.
    pub fn parse(data: &[u8], emit: &mut dyn FnMut(Token<'_>)) {
        let mut tokenizer = Tokenizer::new();
        tokenizer.feed(data, emit);
        tokenizer.finish(emit);
    }

    /// Adds a chunk and emits every token it completes. Large chunks are
    /// processed in small pieces, so the buffer only ever holds an
    /// incomplete token plus one piece.
    pub fn feed(&mut self, data: &[u8], emit: &mut dyn FnMut(Token<'_>)) {
        for mut piece in data.chunks(PIECE) {
            if let Some(skip) = &mut self.skip {
                match skip.end_in(piece) {
                    Some(end) => {
                        piece = &piece[end..];
                        self.skip = None;
                    }
                    None => continue,
                }
            }
            self.pending.extend_from_slice(piece);
            let consumed = process(&self.pending, false, self.html, &mut self.raw, emit);
            self.pending.drain(..consumed);
            // What is left is unfinished markup; text is emitted early.
            if self.pending.len() > MAX_MARKUP {
                self.skip = Some(Skip::start(&self.pending));
                self.pending.clear();
            }
        }
    }

    /// Emits what is left at the end of the input.
    pub fn finish(&mut self, emit: &mut dyn FnMut(Token<'_>)) {
        process(&self.pending, true, self.html, &mut self.raw, emit);
        self.pending.clear();
        self.skip = None;
        self.raw = None;
    }
}

/// What ends overlong markup that is skipped: the same as when it is
/// read (see `process`).
#[derive(Debug, Clone, Copy)]
enum Skip {
    /// A tag ends at a `>` outside quoted values; the quote open, if any.
    Tag(Option<u8>),
    /// Other markup ends with `end`; `seen` are the two bytes before.
    Until { end: &'static [u8], seen: [u8; 2] },
}

impl Skip {
    /// Starts skipping `markup`, the start of markup that is unfinished.
    fn start(markup: &[u8]) -> Skip {
        let until = |end| Skip::Until { end, seen: [0; 2] };
        let (mut skip, opening) = if markup.starts_with(b"<!--") {
            (until(b"-->"), 4)
        } else if markup.starts_with(b"<![CDATA[") {
            (until(b"]]>"), 9)
        } else if markup.starts_with(b"<?") {
            (until(b"?>"), 2)
        } else if markup.starts_with(b"<!") {
            (until(b">"), 2)
        } else {
            (Skip::Tag(None), 1)
        };
        // Takes in the quotes and last bytes; the end is not there, or the
        // markup would be finished.
        let _ = skip.end_in(&markup[opening.min(markup.len())..]);
        skip
    }

    /// Where the markup ends in `bytes` (the index after its end), if it
    /// does there.
    fn end_in(&mut self, bytes: &[u8]) -> Option<usize> {
        for (i, &b) in bytes.iter().enumerate() {
            match self {
                Skip::Tag(quote) => match (*quote, b) {
                    (None, b'"' | b'\'') => *quote = Some(b),
                    (Some(q), b) if b == q => *quote = None,
                    (None, b'>') => return Some(i + 1),
                    _ => {}
                },
                Skip::Until { end, seen } => {
                    if let Some((last, before)) = end.split_last()
                        && b == *last
                        && seen.ends_with(before)
                    {
                        return Some(i + 1);
                    }
                    *seen = [seen[1], b];
                }
            }
        }
        None
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Where the end tag of raw text element `name` starts, in any case.
fn find_end_tag(haystack: &[u8], name: &str) -> Option<usize> {
    let len = name.len() + 2;
    haystack
        .windows(len)
        .position(|w| w.starts_with(b"</") && w[2..].eq_ignore_ascii_case(name.as_bytes()))
}

/// A cut in `run` that splits neither a UTF-8 sequence nor an entity; 0
/// if there is none.
fn safe_cut(run: &[u8]) -> usize {
    // Before the last UTF-8 sequence (at most 4 bytes), which may continue
    // in the next chunk.
    let mut start = run.len();
    while start > 0 && run.len() - start < 3 && run[start - 1] & 0xc0 == 0x80 {
        start -= 1;
    }
    let mut cut = match start.checked_sub(1) {
        Some(lead) if run[lead] >= 0xc0 => lead,
        _ => run.len(),
    };
    // Before an entity that may continue in the next chunk.
    let tail = &run[cut.saturating_sub(12)..cut];
    if let Some(amp) = tail.iter().rposition(|&b| b == b'&')
        && !tail[amp..].contains(&b';')
    {
        cut = cut - tail.len() + amp;
    }
    cut
}

fn text(bytes: &[u8]) -> Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

/// Processes `buf`, returning how many bytes were consumed. Without `last`,
/// incomplete tokens at the end are left for the next round. With `html`,
/// `raw` is the raw text element being read, if any.
fn process(
    buf: &[u8],
    last: bool,
    html: bool,
    raw: &mut Option<&'static str>,
    emit: &mut dyn FnMut(Token<'_>),
) -> usize {
    let mut pos = 0;
    while pos < buf.len() {
        if let Some(name) = *raw {
            let rest = &buf[pos..];
            let end = match find_end_tag(rest, name) {
                Some(end) => {
                    *raw = None;
                    end
                }
                None if last => rest.len(),
                // Keep what could be the start of the end tag.
                None => match rest
                    .len()
                    .checked_sub(name.len() + 2)
                    .map(|keep| safe_cut(&rest[..keep]))
                {
                    Some(cut) if cut > 0 => cut,
                    _ => return pos,
                },
            };
            if end > 0 {
                emit(Token::Text(text(&rest[..end])));
            }
            pos += end;
            continue;
        }
        if buf[pos] != b'<' {
            let end = match buf[pos..].iter().position(|&b| b == b'<') {
                Some(i) => pos + i,
                None if last => buf.len(),
                None => {
                    // Unfinished text: emit the part up to the last
                    // whitespace, which never splits an entity or a UTF-8
                    // sequence; without whitespace, cut at a safe point.
                    let run = &buf[pos..];
                    match run.iter().rposition(|b| b.is_ascii_whitespace()) {
                        Some(i) if i > 0 && run.len() > EAGER_TEXT => pos + i,
                        _ if run.len() > MAX_TEXT => pos + safe_cut(run),
                        _ => return pos,
                    }
                }
            };
            emit(Token::Text(text(&buf[pos..end])));
            pos = end;
            continue;
        }

        let rest = &buf[pos..];
        let complete = |end: Option<usize>, skip: usize| end.map(|e| e + skip);
        let token_end = if rest.starts_with(b"<!--") {
            complete(find(&rest[4..], b"-->"), 4 + 3)
        } else if rest.starts_with(b"<![CDATA[") {
            if let Some(end) = find(&rest[9..], b"]]>") {
                if let Ok(content) = core::str::from_utf8(&rest[9..9 + end]) {
                    emit(Token::CData(content));
                }
                Some(9 + end + 3)
            } else {
                None
            }
        } else if rest.starts_with(b"<?") {
            complete(find(&rest[2..], b"?>"), 2 + 2)
        } else if rest.starts_with(b"<!") {
            complete(rest.iter().position(|&b| b == b'>'), 1)
        } else if rest.get(1).is_some_and(|&b| b != b'/' && !starts_name(b)) {
            // No tag follows (`a < b`, `<3`): the `<` is text.
            emit(Token::Text(Cow::Borrowed("<")));
            Some(1)
        } else {
            match tag_end(rest) {
                Some(end) => {
                    if let Some(name) = emit_tag(&rest[1..end], emit)
                        && html
                    {
                        *raw = RAW_TEXT
                            .into_iter()
                            .find(|raw| local_name(name).eq_ignore_ascii_case(raw));
                    }
                    Some(end + 1)
                }
                None => None,
            }
        };
        match token_end {
            Some(len) => pos += len,
            None if last => {
                // Unterminated markup at the end: treat it as text.
                emit(Token::Text(text(&buf[pos..])));
                return buf.len();
            }
            None => return pos,
        }
    }
    pos
}

/// Whether a name may start with `byte`: a letter, `_`, `:` or a character
/// beyond ASCII.
fn starts_name(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':') || byte >= 0x80
}

/// Index of the `>` closing a tag, ignoring `>` inside quoted values.
fn tag_end(tag: &[u8]) -> Option<usize> {
    let mut quote = None;
    for (i, &b) in tag.iter().enumerate().skip(1) {
        match (quote, b) {
            (None, b'"' | b'\'') => quote = Some(b),
            (Some(q), b) if b == q => quote = None,
            (None, b'>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// Emits a tag; returns the name of an element it opens (not of one that
/// closes itself).
fn emit_tag<'a>(inner: &'a [u8], emit: &mut dyn FnMut(Token<'_>)) -> Option<&'a str> {
    let Ok(inner) = core::str::from_utf8(inner) else {
        return None;
    };
    if let Some(name) = inner.strip_prefix('/') {
        emit(Token::End { name: name.trim() });
        return None;
    }
    let (inner, self_closing) = match inner.trim_end().strip_suffix('/') {
        Some(stripped) => (stripped, true),
        None => (inner, false),
    };
    let name_end = inner
        .find(|c: char| c.is_whitespace())
        .unwrap_or(inner.len());
    let name = &inner[..name_end];
    if name.is_empty() {
        return None;
    }
    emit(Token::Start {
        name,
        attrs: Attrs(&inner[name_end..]),
        self_closing,
    });
    (!self_closing).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;
    use alloc::vec;

    fn describe(token: Token<'_>) -> String {
        match token {
            Token::Start {
                name,
                attrs,
                self_closing,
            } => {
                let attrs: Vec<String> = attrs.iter().map(|(n, v)| format!("{n}={v}")).collect();
                format!(
                    "<{name} {}{}>",
                    attrs.join(","),
                    if self_closing { "/" } else { "" }
                )
            }
            Token::End { name } => format!("</{name}>"),
            Token::Text(t) => format!("'{t}'"),
            Token::CData(t) => format!("cdata '{t}'"),
        }
    }

    fn tokens(data: &str, chunk: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut tokenizer = Tokenizer::new();
        for piece in data.as_bytes().chunks(chunk) {
            tokenizer.feed(piece, &mut |t| out.push(describe(t)));
        }
        tokenizer.finish(&mut |t| out.push(describe(t)));
        // Merge adjacent text pieces for comparison.
        let mut merged: Vec<String> = Vec::new();
        for t in out {
            match (merged.last_mut(), t.starts_with('\'')) {
                (Some(last), true) if last.starts_with('\'') => {
                    last.pop();
                    last.push_str(&t[1..]);
                }
                _ => merged.push(t),
            }
        }
        merged
    }

    const DOC: &str = "<?xml version=\"1.0\"?><!DOCTYPE html><html xmlns=\"x\">\
        <!-- a > comment --><p class=\"a b\" id='x' hidden>Hello &amp; \
        <em>world</em>!<br/></p><![CDATA[1 < 2]]><img alt=\"a > b\" /></html>";

    #[test]
    fn html_scripts_and_styles_are_text() {
        let page = "<p>a<script>if (a<b && c>d) { x('</div>'); }</SCRIPT>\
            <style>p > b { }</style>b</p><script/>c";
        for chunk in [1, 3, 7, 64, 1000] {
            let mut out = Vec::new();
            let mut tokenizer = Tokenizer::html();
            for piece in page.as_bytes().chunks(chunk) {
                tokenizer.feed(piece, &mut |t| out.push(describe(t)));
            }
            tokenizer.finish(&mut |t| out.push(describe(t)));
            let mut merged: Vec<String> = Vec::new();
            for t in out {
                match (merged.last_mut(), t.starts_with('\'')) {
                    (Some(last), true) if last.starts_with('\'') => {
                        last.pop();
                        last.push_str(&t[1..]);
                    }
                    _ => merged.push(t),
                }
            }
            assert_eq!(
                merged,
                [
                    "<p >",
                    "'a'",
                    "<script >",
                    "'if (a<b && c>d) { x('</div>'); }'",
                    "</SCRIPT>",
                    "<style >",
                    "'p > b { }'",
                    "</style>",
                    "'b'",
                    "</p>",
                    "<script />",
                    "'c'"
                ],
                "chunk {chunk}"
            );
        }
    }

    #[test]
    fn tokenizes_regardless_of_chunk_size() {
        let expected = vec![
            "<html xmlns=x>",
            "<p class=a b,id=x>",
            "'Hello &amp; '",
            "<em >",
            "'world'",
            "</em>",
            "'!'",
            "<br />",
            "</p>",
            "cdata '1 < 2'",
            "<img alt=a > b/>",
            "</html>",
        ];
        for chunk in [1, 3, 7, 64, 1000] {
            assert_eq!(tokens(DOC, chunk), expected, "chunk {chunk}");
        }
    }

    #[test]
    fn attributes_are_decoded_on_request() {
        let mut found = None;
        Tokenizer::parse(
            b"<a href=\"x.html#c&amp;d\" title='It&#8217;s'>",
            &mut |t| {
                if let Token::Start { attrs, .. } = t {
                    found = Some((
                        attrs.get("href").map(|v| v.to_string()),
                        attrs.get("title").map(|v| v.to_string()),
                        attrs.get("missing").map(|v| v.to_string()),
                    ));
                }
            },
        );
        assert_eq!(
            found,
            Some((
                Some("x.html#c&d".to_string()),
                Some("It\u{2019}s".to_string()),
                None
            ))
        );
    }

    #[test]
    fn malformed_markup_degrades_to_text() {
        assert_eq!(tokens("a < b", 2), ["'a < b'"]);
        assert_eq!(local_name("dc:title"), "title");
        assert_eq!(local_name("title"), "title");
    }

    /// Feeds `data` in chunks; returns the decoded text and the largest
    /// buffer the tokenizer needed.
    fn text_of(data: &str, chunk: usize) -> (String, usize) {
        let mut text = String::new();
        let mut most = 0;
        let mut tokenizer = Tokenizer::new();
        let mut emit = |token: Token<'_>| {
            if let Token::Text(t) = token {
                text.push_str(&decode(&t));
            }
        };
        for piece in data.as_bytes().chunks(chunk) {
            tokenizer.feed(piece, &mut emit);
            most = most.max(tokenizer.pending.capacity());
        }
        tokenizer.finish(&mut emit);
        (text, most)
    }

    #[test]
    fn long_text_streams_through_a_small_buffer() {
        let paragraph = "Wörter &amp; Zeichen ".repeat(400);
        let unbroken = alloc::format!("{}&amp;{}", "ä".repeat(1500), "ö".repeat(700));
        let data = alloc::format!("<p>{paragraph}</p><p>{unbroken}</p>");
        let expected = alloc::format!("{}{}", decode(&paragraph), decode(&unbroken));
        for chunk in [1, 7, 100, 5000] {
            let (text, most) = text_of(&data, chunk);
            assert_eq!(text, expected, "chunk {chunk}");
            assert!(most <= 4096, "chunk {chunk}: buffer {most}");
        }
    }

    #[test]
    fn overlong_markup_is_skipped_to_its_own_end() {
        let long = "x > y ".repeat(1000);
        for markup in [
            alloc::format!("<!-- {long} -->"),
            alloc::format!("<img alt=\"{long}\" title='>'/>"),
            alloc::format!("<?pi {long}?>"),
            alloc::format!("<![CDATA[{long}]]>"),
        ] {
            let data = alloc::format!("<p>a</p>{markup}<p>b</p>");
            for chunk in [1, 100, 5000] {
                let (text, most) = text_of(&data, chunk);
                assert_eq!(text, "ab", "{}, chunk {chunk}", &markup[..4]);
                assert!(most <= 8192, "chunk {chunk}: buffer {most}");
            }
        }
    }

    #[test]
    fn less_than_without_a_name_is_text() {
        for chunk in [1, 3, 64] {
            assert_eq!(
                tokens("a < b <em>x</em> 1 <3 <= 2<", chunk),
                ["'a < b '", "<em >", "'x'", "</em>", "' 1 <3 <= 2<'"],
                "chunk {chunk}"
            );
        }
    }

    #[test]
    fn raw_text_keeps_characters_whole() {
        let page = "<script>äöü€ abc</script>";
        for chunk in [1, 2, 3] {
            let mut text = String::new();
            let mut tokenizer = Tokenizer::html();
            let mut emit = |token: Token<'_>| {
                if let Token::Text(t) = token {
                    text.push_str(&t);
                }
            };
            for piece in page.as_bytes().chunks(chunk) {
                tokenizer.feed(piece, &mut emit);
            }
            tokenizer.finish(&mut emit);
            assert_eq!(text, "äöü€ abc", "chunk {chunk}");
        }
    }

    #[test]
    fn overlong_markup_is_skipped() {
        let image = "A".repeat(20_000);
        let data = alloc::format!("<p>a</p><img src=\"data:image/png;base64,{image}\"/><p>b</p>");
        for chunk in [100, 5000] {
            let (text, most) = text_of(&data, chunk);
            assert_eq!(text, "ab");
            assert!(most <= 8192, "chunk {chunk}: buffer {most}");
        }
    }
}
