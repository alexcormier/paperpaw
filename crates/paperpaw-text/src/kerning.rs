//! Kerning read from OpenType fonts: the pair adjustments of the GPOS
//! `kern` feature (pair positioning, formats 1 and 2: how much the first
//! glyph's advance changes), or the legacy `kern` table where a font has
//! no GPOS kerning. Variable fonts get the adjustments of their instance.

use read_fonts::tables::gpos::{DeviceOrVariationIndex, PairPos, PositionSubtables, ValueRecord};
use read_fonts::tables::kern::SubtableKind;
use read_fonts::tables::layout::LangSys;
use read_fonts::tables::variations::{DeltaSetIndex, ItemVariationStore};
use read_fonts::types::{F2Dot14, GlyphId, GlyphId16, Tag};
use read_fonts::{FontRef, TableProvider};

const KERN: Tag = Tag::new(b"kern");

/// The script whose kerning a pair of characters takes: Cyrillic or
/// Greek where either is, else Latin (also for digits and punctuation).
fn script(left: char, right: char) -> Tag {
    let of = |ch: char| match ch {
        '\u{400}'..='\u{52f}' | '\u{1c80}'..='\u{1c8f}' => Some(Tag::new(b"cyrl")),
        '\u{370}'..='\u{3ff}' | '\u{1f00}'..='\u{1fff}' => Some(Tag::new(b"grek")),
        _ => None,
    };
    of(left).or_else(|| of(right)).unwrap_or(Tag::new(b"latn"))
}

/// The kerning of one font instance.
pub(crate) struct Kerning<'a> {
    font: FontRef<'a>,
    /// The instance's normalized coordinates, for variable fonts.
    coords: &'a [F2Dot14],
    /// Where the adjustments of a variable font vary.
    store: Option<ItemVariationStore<'a>>,
    /// Whether the font kerns with GPOS: then its legacy table is not
    /// read (both are often there, the same pairs twice).
    gpos: bool,
}

impl<'a> Kerning<'a> {
    pub(crate) fn new(font: FontRef<'a>, coords: &'a [F2Dot14]) -> Self {
        let store = match coords.is_empty() {
            true => None,
            false => font
                .gdef()
                .ok()
                .and_then(|gdef| gdef.item_var_store())
                .and_then(Result::ok),
        };
        let gpos = font
            .gpos()
            .ok()
            .and_then(|g| g.feature_list().ok())
            .is_some_and(|list| {
                list.feature_records()
                    .iter()
                    .any(|r| r.feature_tag() == KERN)
            });
        Self {
            font,
            coords,
            store,
            gpos,
        }
    }

    /// Whether the font kerns nothing: no GPOS kerning, no legacy table.
    pub(crate) fn is_empty(&self) -> bool {
        !self.gpos && self.font.kern().is_err()
    }

    /// The language system whose features apply to `script`: its own,
    /// else the default script's, else the first.
    fn lang_sys(&self, script: Tag) -> Option<LangSys<'a>> {
        let list = self.font.gpos().ok()?.script_list().ok()?;
        let index = list
            .index_for_tag(script)
            .or_else(|| list.index_for_tag(Tag::new(b"DFLT")))
            .or_else(|| (list.script_count() > 0).then_some(0))?;
        list.get(index).ok()?.element.default_lang_sys()?.ok()
    }

    /// The adjustment of the pair `left`, `right` (the characters, for
    /// the script, and their glyphs) in font units; 0 for none.
    pub(crate) fn pair(&self, chars: (char, char), left: GlyphId, right: GlyphId) -> f32 {
        if self.gpos {
            self.gpos_pair(script(chars.0, chars.1), left, right)
        } else {
            self.legacy_pair(left, right)
        }
    }

    fn gpos_pair(&self, script: Tag, left: GlyphId, right: GlyphId) -> f32 {
        let (Ok(left), Ok(right)) = (
            u16::try_from(left.to_u32()).map(GlyphId16::new),
            u16::try_from(right.to_u32()).map(GlyphId16::new),
        ) else {
            return 0.0;
        };
        let (Some(lang_sys), Ok(gpos)) = (self.lang_sys(script), self.font.gpos()) else {
            return 0.0;
        };
        let (Ok(features), Ok(lookups)) = (gpos.feature_list(), gpos.lookup_list()) else {
            return 0.0;
        };
        let mut total = 0.0;
        for index in lang_sys.feature_indices() {
            let Ok(feature) = features.get(index.get()) else {
                continue;
            };
            if feature.tag != KERN {
                continue;
            }
            for lookup in feature.element.lookup_list_indices() {
                let Ok(lookup) = lookups.lookups().get(usize::from(lookup.get())) else {
                    continue;
                };
                if let Ok(PositionSubtables::Pair(subtables)) = lookup.subtables() {
                    total += self.lookup_pair(subtables.iter().flatten(), left, right);
                }
            }
        }
        total
    }

    /// The adjustment one lookup makes: of its first subtable that holds
    /// the pair (format 2 holds every pair whose first glyph it covers).
    fn lookup_pair(
        &self,
        subtables: impl Iterator<Item = PairPos<'a>>,
        left: GlyphId16,
        right: GlyphId16,
    ) -> f32 {
        for subtable in subtables {
            match subtable {
                PairPos::Format1(table) => {
                    let Some(index) = table.coverage().ok().and_then(|c| c.get(left)) else {
                        continue;
                    };
                    let Ok(set) = table.pair_sets().get(usize::from(index)) else {
                        continue;
                    };
                    if let Some(record) = set.find_pair_value_record(right) {
                        return self.x_advance(record.value_record1());
                    }
                }
                PairPos::Format2(table) => {
                    if table.coverage().ok().and_then(|c| c.get(left)).is_none() {
                        continue;
                    }
                    let (Ok(first), Ok(second)) = (table.class_def1(), table.class_def2()) else {
                        continue;
                    };
                    let records = table.class_value_records();
                    return records
                        .get(first.get(left), second.get(right))
                        .map_or(0.0, |[value, _]| self.x_advance(&value));
                }
            }
        }
        0.0
    }

    /// How much a value record changes the advance, at the instance.
    fn x_advance(&self, value: &ValueRecord<'a>) -> f32 {
        let base = f32::from(value.x_advance().unwrap_or(0));
        let delta = match (&self.store, value.x_advance_device()) {
            (Some(store), Some(Ok(DeviceOrVariationIndex::VariationIndex(index)))) => {
                let index = DeltaSetIndex {
                    outer: index.delta_set_outer_index(),
                    inner: index.delta_set_inner_index(),
                };
                store
                    .compute_delta(index, self.coords)
                    .map_or(0.0, |d| d.to_f64() as f32)
            }
            _ => 0.0,
        };
        base + delta
    }

    /// The legacy table's adjustment: of its horizontal subtables.
    fn legacy_pair(&self, left: GlyphId, right: GlyphId) -> f32 {
        let Ok(kern) = self.font.kern() else {
            return 0.0;
        };
        kern.subtables()
            .flatten()
            .filter(|s| s.is_horizontal() && !s.is_cross_stream() && !s.is_variable())
            .filter_map(|s| match s.kind().ok()? {
                SubtableKind::Format0(t) => t.kerning(left, right),
                SubtableKind::Format2(t) => t.kerning(left, right),
                SubtableKind::Format3(t) => t.kerning(left, right),
                SubtableKind::Format1(_) => None,
            })
            .map(|v| v as f32)
            .sum()
    }
}
