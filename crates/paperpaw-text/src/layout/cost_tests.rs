//! The optional line cost: compatibility, space counts and bounded work.

use super::*;
use core::cell::RefCell;

#[test]
fn zero_or_invalid_extra_cost_keeps_the_existing_breaks() {
    let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| {
        for (i, _) in word.char_indices().skip(2).take(4) {
            out(i);
        }
    };
    for text in [
        "",
        "  aa  bbb cccc\n\nnext line",
        "well-known Wörter abcdefghijkl end",
        "aa bb cc ddddd eee ff ggggg",
    ] {
        for width in [1.0, 7.0, 12.0] {
            let old = wrap_lines_balanced_kerned(
                text,
                |_| width,
                |_, _| 1.0,
                |_, _, _| 0.0,
                Some(&hyphenate),
            );
            for cost in [0.0, -1.0, f32::NAN] {
                let scored = wrap_lines_balanced_kerned_with_cost(
                    text,
                    |_| width,
                    |_, _| 1.0,
                    |_, _, _| 0.0,
                    Some(&hyphenate),
                    |_, _| cost,
                );
                assert_eq!(scored, old, "{text:?}, width {width}, cost {cost}");
            }
        }
    }
}

#[test]
fn the_cost_counts_internal_spaces_and_skips_final_lines() {
    let seen = RefCell::new(Vec::new());
    let text = "  aa  bbb cccc";
    let lines = wrap_lines_balanced_kerned_with_cost(
        text,
        |_| 7.0,
        |_, _| 1.0,
        |_, _, _| 0.0,
        None,
        |free, spaces| {
            seen.borrow_mut().push((free, spaces));
            0.0
        },
    );
    // The only non-final candidates: "aa", "aa  bbb", and "bbb".
    assert_eq!(*seen.borrow(), [(5.0, 0), (4.0, 0), (0.0, 2)]);
    assert_eq!(lines[0].spaces, 2);
    assert_eq!(&text[lines[0].range.clone()], "aa  bbb");
    assert!(lines[1].last);
}

#[test]
fn prefix_counts_cover_spaces_dashes_and_hyphenation() {
    let text = "  aa  Wörter-abc defghijkl  zz";
    // Actual candidate ends, including cuts inside the same word.
    let word = text.find("defghijkl").unwrap();
    let dash = text.find('-').unwrap() + 1;
    let points = [
        (2, 2),
        (4, 6),
        (dash, dash),
        (word - 1, word),
        (word + 3, word + 3),
        (word + 9, word + 11),
        (text.len(), text.len()),
    ];
    let mut breaks: Vec<Break> = points
        .into_iter()
        .map(|(end, next)| Break {
            end: end as u16,
            next: next as u16,
            to_end: 0.0,
            to_next: 0.0,
            hyphen: 0.0,
            cost: 0.0,
            prev: 0,
            spaces: 0,
        })
        .collect();
    count_spaces(text, 2, &mut breaks);
    for (j, end) in breaks.iter().enumerate().skip(1) {
        for start in &breaks[..j] {
            let actual = text[usize::from(start.next)..usize::from(end.end)]
                .bytes()
                .filter(|&b| b == b' ')
                .count();
            assert_eq!(
                usize::from(end.spaces - start.spaces - (start.next - start.end)),
                actual
            );
        }
    }
    assert_eq!(core::mem::size_of::<Break>(), 24, "no larger break buffer");
}

#[test]
fn too_many_breaks_use_the_bounded_greedy_fallback() {
    let text = "a ".repeat(MAX_BREAKS + 1);
    let scored = wrap_lines_balanced_kerned_with_cost(
        &text,
        |_| 20.0,
        |_, _| 1.0,
        |_, _, _| 0.0,
        None,
        |_, _| panic!("oversized paragraphs must not score every pair"),
    );
    let greedy = wrap_lines_kerned(&text, |_| 20.0, |_, _| 1.0, |_, _, _| 0.0, None);
    assert_eq!(scored, greedy);
}
