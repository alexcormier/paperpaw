//! Turning StarDict definition data into plain text.

use alloc::string::String;

/// The text of a definition's fields, typed by `types` (the dictionary's
/// `sametypesequence`) or by a type byte before each field. Each field is
/// decoded straight into the text, so little more memory is needed than
/// the text itself.
pub(crate) fn decode(data: &[u8], types: Option<&str>) -> String {
    let mut text = String::new();
    let mut rest = data;
    match types {
        Some(types) => {
            let types = types.as_bytes();
            for (i, &kind) in types.iter().enumerate() {
                if rest.is_empty() {
                    break;
                }
                let (field, after) = split_field(rest, kind, i + 1 == types.len());
                push_field(&mut text, kind, field);
                rest = after;
            }
        }
        None => {
            while let Some((&kind, after)) = rest.split_first() {
                let (field, after) = split_field(after, kind, false);
                push_field(&mut text, kind, field);
                rest = after;
            }
        }
    }
    text
}

/// Appends the text of a field on a line of its own, if it has any.
fn push_field(text: &mut String, kind: u8, field: &[u8]) {
    let start = text.len();
    if start > 0 {
        text.push('\n');
    }
    let body = text.len();
    match kind {
        b'h' | b'g' | b'x' | b'k' | b'w' | b'r' => push_plain_text(text, chars(field)),
        b't' => {
            text.push('[');
            push_trimmed(text, field);
            if text.len() > body + 1 {
                text.push(']');
            } else {
                text.truncate(body);
            }
        }
        b'm' | b'l' | b'y' => push_trimmed(text, field),
        // Binary data (sounds, pictures) and unknown types.
        _ => {}
    }
    if text.len() == body {
        text.truncate(start);
    }
}

/// Appends `field` without the white space around it.
fn push_trimmed(text: &mut String, field: &[u8]) {
    let start = text.len();
    text.extend(chars(field).skip_while(|c| c.is_whitespace()));
    let end = start + text[start..].trim_end().len();
    text.truncate(end);
}

/// The characters of `bytes` as `String::from_utf8_lossy` gives them,
/// without a copy.
fn chars(bytes: &[u8]) -> impl Iterator<Item = char> + '_ {
    bytes.utf8_chunks().flat_map(|chunk| {
        let invalid = (!chunk.invalid().is_empty()).then_some(char::REPLACEMENT_CHARACTER);
        chunk.valid().chars().chain(invalid)
    })
}

/// One field: text up to a NUL byte (or the rest, for the last field),
/// binary data with a 4-byte size before it.
fn split_field(data: &[u8], kind: u8, last: bool) -> (&[u8], &[u8]) {
    if kind.is_ascii_uppercase() {
        if last {
            return (data, &[]);
        }
        let Some((size, rest)) = data.split_first_chunk::<4>() else {
            return (&[], &[]);
        };
        return split_sized(rest, u32::from_be_bytes(*size) as usize);
    }
    if last {
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        return (&data[..end], &[]);
    }
    match data.iter().position(|&b| b == 0) {
        Some(end) => (&data[..end], &data[end + 1..]),
        None => (data, &[]),
    }
}

/// The first `size` bytes of `data` (all if it is shorter) and the rest.
fn split_sized(data: &[u8], size: usize) -> (&[u8], &[u8]) {
    data.split_at(size.min(data.len()))
}

/// Plain text from simple markup (HTML, XDXF, Pango): tags are dropped,
/// block elements start new lines, list items get a bullet, entities are
/// decoded and white space is collapsed.
pub fn to_plain_text(markup: &str) -> String {
    let mut text = String::new();
    push_plain_text(&mut text, markup.chars());
    text
}

/// Appends the plain text of `markup` (see [`to_plain_text`]) to `text`.
fn push_plain_text(text: &mut String, markup: impl Iterator<Item = char>) {
    let mut out = Collapse {
        text,
        any: false,
        word: false,
        space: false,
    };
    let mut chars = markup.peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let tag = Tag::read(&mut chars);
                match tag.name() {
                    b"br" | b"p" | b"div" | b"tr" | b"ol" | b"ul" | b"dd" | b"dt"
                    | b"blockquote" | b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" | b"def"
                    | b"ex" => out.push('\n'),
                    b"li" if !tag.closing => out.push_str("\n• "),
                    _ => {}
                }
            }
            '&' => {
                let mut buf = [0u8; 16];
                let mut len = 0;
                while let Some(&next) = chars.peek() {
                    if next == ';' || len > 8 {
                        break;
                    }
                    len += next.encode_utf8(&mut buf[len..]).len();
                    chars.next();
                }
                let entity = core::str::from_utf8(&buf[..len]).unwrap_or_default();
                if chars.next_if_eq(&';').is_some() {
                    match entity_char(entity) {
                        Some(c) => out.push(c),
                        None => {
                            out.push('&');
                            out.push_str(entity);
                            out.push(';');
                        }
                    }
                } else {
                    out.push('&');
                    out.push_str(entity);
                }
            }
            '\r' | '\n' | '\t' => out.push(' '),
            c => out.push(c),
        }
    }
}

/// Writes text with its white space collapsed: words separated by single
/// spaces, lines (started by `'\n'`) trimmed, empty lines dropped.
struct Collapse<'a> {
    text: &'a mut String,
    /// Whether a word has been written.
    any: bool,
    /// Whether the current line has a word.
    word: bool,
    /// Whether a space follows the last word.
    space: bool,
}

impl Collapse<'_> {
    fn push(&mut self, c: char) {
        match c {
            '\n' => (self.word, self.space) = (false, false),
            ' ' => self.space = self.word,
            c => {
                if !self.word && self.any {
                    self.text.push('\n');
                } else if self.space {
                    self.text.push(' ');
                }
                self.text.push(c);
                (self.any, self.word, self.space) = (true, true, false);
            }
        }
    }

    fn push_str(&mut self, s: &str) {
        s.chars().for_each(|c| self.push(c));
    }
}

/// What matters of a tag: whether it is a closing tag, and its name.
struct Tag {
    closing: bool,
    /// The name in lower case, if no longer than the longest name looked
    /// for.
    name: [u8; 10],
    len: usize,
    too_long: bool,
}

impl Tag {
    /// Reads a tag after its `<`, up to and with its `>`.
    fn read(chars: &mut impl Iterator<Item = char>) -> Tag {
        let mut tag = Tag {
            closing: false,
            name: [0; 10],
            len: 0,
            too_long: false,
        };
        let (mut first, mut naming) = (true, true);
        for c in chars.by_ref() {
            if c == '>' {
                break;
            }
            if first {
                tag.closing = c == '/';
                first = false;
            }
            // Slashes before the name are skipped.
            if !naming || (c == '/' && tag.len == 0 && !tag.too_long) {
                continue;
            }
            if !c.is_ascii_alphanumeric() {
                naming = false;
            } else if let Some(byte) = tag.name.get_mut(tag.len) {
                *byte = c.to_ascii_lowercase() as u8;
                tag.len += 1;
            } else {
                tag.too_long = true;
            }
        }
        tag
    }

    fn name(&self) -> &[u8] {
        if self.too_long {
            &[]
        } else {
            &self.name[..self.len]
        }
    }
}

/// The character an entity stands for.
fn entity_char(entity: &str) -> Option<char> {
    let c = match entity {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "laquo" => '«',
        "raquo" => '»',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hellip" => '…',
        "middot" => '·',
        _ => {
            let code = if let Some(hex) = entity.strip_prefix("#x").or(entity.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                entity.strip_prefix('#').and_then(|d| d.parse().ok())
            };
            return code.and_then(char::from_u32);
        }
    };
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_plain_text() {
        let html = "<p><b>hus</b> <i>Substantiv</i></p><ol><li>das Haus</li><li>das &laquo;Heim&raquo; &amp; Co</li></ol>";
        assert_eq!(
            to_plain_text(html),
            "hus Substantiv\n• das Haus\n• das «Heim» & Co"
        );
        assert_eq!(to_plain_text("a&#228;b&#xF6;c"), "aäböc");
    }

    #[test]
    fn white_space_and_odd_markup() {
        assert_eq!(to_plain_text("  a \t b\r\n<br/>\n<BR >  c  "), "a b\nc");
        assert_eq!(to_plain_text("<P>x</p><//div>y<li></li>"), "x\ny\n•");
        // Unknown entities and unclosed ones stay as they are; a tag name
        // longer than any known one is no block element.
        assert_eq!(
            to_plain_text("&bogus; &amp &#xD800;"),
            "&bogus; &amp &#xD800;"
        );
        assert_eq!(to_plain_text("a<blockquotes>b<blockquote>c"), "ab\nc");
        assert_eq!(to_plain_text("a&nbsp;&nbsp;b"), "a b");
    }

    #[test]
    fn fields_by_type() {
        assert_eq!(decode(b"tone\0<b>Ton</b>", Some("th")), "[tone]\nTon");
        assert_eq!(
            decode(b"mfirst\0hsecond<br>line\0", None),
            "first\nsecond\nline"
        );
        // Empty fields, binary fields and invalid UTF-8.
        assert_eq!(decode(b" \0 a \0", Some("tmm")), "a");
        assert_eq!(decode(b"W\0\0\0\x02\xff\xffm\xffok\0", None), "\u{fffd}ok");
    }

    #[test]
    fn binary_field_size_does_not_wrap() {
        // On the X4 `usize` has 32 bits: 4 + 0xffff_ffff used to wrap.
        assert_eq!(split_sized(b"abc", usize::MAX), (&b"abc"[..], &b""[..]));
        assert_eq!(split_sized(b"abc", usize::MAX - 2), (&b"abc"[..], &b""[..]));
        assert_eq!(
            split_field(b"\xff\xff\xff\xffabc", b'W', false),
            (&b"abc"[..], &b""[..])
        );
        assert_eq!(decode(b"W\xff\xff\xff\xffabc", None), "");
    }

    #[test]
    fn many_types_are_cheap() {
        let types = "m".repeat(16 * 1024);
        assert_eq!(decode(b"a\0b", Some(&types)), "a\nb");
    }
}
