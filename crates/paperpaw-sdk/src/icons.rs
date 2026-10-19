//! Icons by name, e.g. `books` or `settings`, drawn from the icon font
//! bound to [`fonts::ICONS`](crate::builtin::fonts::ICONS).
//!
//! The names are those of [Tabler Icons](https://tabler.io/icons), all of
//! them known here; the icon font holds the drawings of those PaperPaw
//! uses (see `tools/icons/subset.py`), icon packs add others (font packs
//! that extend it, see `tools/icons/pack.py`). An icon theme is a font
//! with other drawings at the same code points, bound to the role in
//! place of the default.
//!
//! Plugins give their menu entries icons by name
//! ([`Registrar::add_icon`](crate::Registrar::add_icon)); an icon without
//! a drawing draws nothing.

include!("icon_table.rs");

/// The name of icon `index` (in the order of [`ICONS`]).
fn name(index: usize) -> &'static str {
    let start = index.checked_sub(1).map_or(0, |i| ICONS[i].0 as usize);
    &NAMES[start..ICONS[index].0 as usize]
}

/// The character of icon `name` in the icon font.
pub fn char(name: &str) -> Option<char> {
    let (mut low, mut high) = (0, ICONS.len());
    while low < high {
        let middle = (low + high) / 2;
        match self::name(middle).cmp(name) {
            core::cmp::Ordering::Less => low = middle + 1,
            core::cmp::Ordering::Greater => high = middle,
            core::cmp::Ordering::Equal => return Some(ICONS[middle].1),
        }
    }
    None
}

/// The names of all icons, sorted.
pub fn names() -> impl Iterator<Item = &'static str> {
    (0..ICONS.len()).map(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_found_by_name() {
        let all: alloc::vec::Vec<&str> = names().collect();
        assert!(all.windows(2).all(|w| w[0] < w[1]), "sorted");
        assert_eq!(char("home"), Some('\u{eac1}'));
        assert!(char("books").is_some());
        // The first and the last.
        assert_eq!(char(all[0]), Some(ICONS[0].1));
        assert_eq!(char(all[all.len() - 1]), Some(ICONS[ICONS.len() - 1].1));
        assert_eq!(char("no-such-icon"), None);
        assert_eq!(char(""), None);
        assert!(all.len() > 5000);
    }
}
