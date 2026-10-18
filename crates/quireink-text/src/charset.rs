//! Character sets for bitmap font packs.

use core::ops::RangeInclusive;

/// Latin script for Western, Central and Northern European languages,
/// plus common typographic punctuation.
pub const LATIN: &[RangeInclusive<char>] = &[
    ' '..='~',
    '\u{a0}'..='\u{17f}',    // Latin-1 Supplement, Latin Extended-A
    '\u{2010}'..='\u{2027}', // dashes, quotes, bullets, ellipsis
    '\u{2030}'..='\u{203a}', // per mille, primes, guillemets
    '\u{20ac}'..='\u{20ac}', // euro sign
    '\u{2122}'..='\u{2122}', // trade mark
    '\u{2212}'..='\u{2212}', // minus sign
    '\u{fffd}'..='\u{fffd}', // replacement character
];

/// Cyrillic (Russian, Ukrainian, Bulgarian, Serbian and more), for packs
/// added to fonts made for Latin.
#[allow(clippy::single_range_in_vec_init)] // a set of ranges, of one here
pub const CYRILLIC: &[RangeInclusive<char>] = &['\u{400}'..='\u{4ff}'];

/// Greek, modern and polytonic (Greek Extended, for older texts).
pub const GREEK: &[RangeInclusive<char>] = &['\u{370}'..='\u{3ff}', '\u{1f00}'..='\u{1fff}'];

/// A character set by name: `latin`, `cyrillic` or `greek`.
pub fn named(name: &str) -> Option<&'static [RangeInclusive<char>]> {
    match name {
        "latin" => Some(LATIN),
        "cyrillic" => Some(CYRILLIC),
        "greek" => Some(GREEK),
        _ => None,
    }
}

/// All characters of `ranges`.
pub fn chars(ranges: &[RangeInclusive<char>]) -> impl Iterator<Item = char> + Clone + '_ {
    ranges.iter().flat_map(|range| range.clone())
}
