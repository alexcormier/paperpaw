//! The report descriptor ("report map") of a device, reduced to the input
//! fields that give keys.

use alloc::vec::Vec;

use crate::HidError;
use crate::key::{Key, PAGE_CONSUMER, PAGE_KEYBOARD};

/// The most bits of one input report (1 KB, more than any ATT value).
const MAX_REPORT_BITS: u32 = 8 * 1024;
/// Nested `Push` items.
const MAX_PUSH: usize = 4;
/// Entries of a usage list kept per field.
const MAX_USAGES: usize = 8192;

/// How an array field's value names a usage.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Lookup {
    /// `first` plus the value's distance from the logical minimum.
    Range { first: u32 },
    /// The value's distance from the logical minimum indexes the list.
    List(Vec<u32>),
}

/// An input field that gives keys.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Field {
    /// One bit or number that says whether its key is down.
    Variable { offset: u32, size: u8, key: Key },
    /// Slots that each hold the usage of a key that is down.
    Array {
        offset: u32,
        size: u8,
        count: u16,
        min: i64,
        max: i64,
        lookup: Lookup,
    },
}

/// The fields of one input report id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub(crate) id: u8,
    fields: Vec<Field>,
    /// Bits the report must have for its fields to be read.
    bits: u32,
}

impl Report {
    /// The keys down in `data`, or `None` if it is too short or a
    /// keyboard says error roll-over (then it says nothing).
    pub(crate) fn keys(&self, data: &[u8]) -> Option<Vec<Key>> {
        if (data.len() as u64) * 8 < u64::from(self.bits) {
            return None;
        }
        let mut keys = Vec::new();
        for field in &self.fields {
            field.keys(data, &mut keys)?;
        }
        Some(keys)
    }
}

impl Field {
    /// Adds the keys down in this field; `None` on error roll-over.
    fn keys(&self, data: &[u8], keys: &mut Vec<Key>) -> Option<()> {
        let (offset, size, count, min, max, lookup) = match self {
            Field::Variable { offset, size, key } => {
                if read_bits(data, *offset, *size) != 0 {
                    keys.push(*key);
                }
                return Some(());
            }
            Field::Array {
                offset,
                size,
                count,
                min,
                max,
                lookup,
            } => (offset, size, count, min, max, lookup),
        };
        for slot in 0..u32::from(*count) {
            let value = i64::from(read_bits(data, offset + slot * u32::from(*size), *size));
            if value < *min || value > *max {
                continue;
            }
            let usage = match lookup {
                Lookup::Range { first } => first.saturating_add((value - min) as u32),
                Lookup::List(list) => match list.get((value - min) as usize) {
                    Some(usage) => *usage,
                    None => continue,
                },
            };
            if usage == (PAGE_KEYBOARD << 16) | 1 {
                return None;
            }
            if let Some(key) = Key::from_usage(usage) {
                keys.push(key);
            }
        }
        Some(())
    }
}

/// The bits `offset..offset + size` of `data`, least significant first.
/// The caller has checked that they are there.
fn read_bits(data: &[u8], offset: u32, size: u8) -> u32 {
    let mut value = 0u32;
    for i in 0..u32::from(size) {
        let bit = offset + i;
        let byte = data.get((bit / 8) as usize).copied().unwrap_or(0);
        value |= u32::from((byte >> (bit % 8)) & 1) << i;
    }
    value
}

/// What a descriptor says about the input reports of a device, as far as
/// keys go: which bits of which report id are keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportMap {
    pub(crate) reports: Vec<Report>,
}

impl ReportMap {
    /// Reads a report descriptor, the value of a HID service's Report
    /// Map characteristic.
    ///
    /// Keys come from the keyboard/keypad page (arrays of keycodes and
    /// bitmaps; modifier keys are not keys here) and the consumer page
    /// (volume, mute, play/pause, next, previous, as arrays or bitmaps).
    /// Everything else is skipped, but its bits count for the offsets.
    /// Output and feature items have their own reports and do not.
    pub fn parse(descriptor: &[u8]) -> Result<ReportMap, HidError> {
        let mut parser = Parser::default();
        let mut rest = descriptor;
        while let Some((&prefix, tail)) = rest.split_first() {
            rest = parser.item(prefix, tail)?;
        }
        Ok(ReportMap {
            reports: parser.reports,
        })
    }

    /// The map of a boot protocol keyboard: modifiers, a reserved byte
    /// and six keycodes, without a report id.
    pub(crate) fn boot_keyboard() -> ReportMap {
        let field = Field::Array {
            offset: 16,
            size: 8,
            count: 6,
            min: 0,
            max: 255,
            lookup: Lookup::Range {
                first: PAGE_KEYBOARD << 16,
            },
        };
        ReportMap {
            reports: alloc::vec![Report {
                id: 0,
                fields: alloc::vec![field],
                bits: 64,
            }],
        }
    }

    /// The fields of input report `id` (0 for none).
    pub(crate) fn report(&self, id: u8) -> Option<&Report> {
        self.reports.iter().find(|r| r.id == id)
    }
}

/// A usage as an item gave it.
#[derive(Debug, Clone, Copy)]
struct RawUsage {
    value: u32,
    /// Four bytes: the page is in the upper 16 bits.
    extended: bool,
}

impl RawUsage {
    fn resolve(self, page: u32) -> u32 {
        if self.extended {
            self.value
        } else {
            (page << 16) | (self.value & 0xFFFF)
        }
    }
}

/// What the local items said, up to the next main item.
#[derive(Debug, Clone, Copy)]
enum Local {
    Usage(RawUsage),
    Range(RawUsage, RawUsage),
}

/// The global items in effect.
#[derive(Debug, Clone, Copy, Default)]
struct Globals {
    page: u32,
    min: i64,
    /// The maximum read as signed and as unsigned (see `max`).
    max_signed: i64,
    max_unsigned: i64,
    size: u32,
    count: u32,
    id: u8,
}

impl Globals {
    /// The logical maximum; a maximum such as 0xFF in one byte is meant
    /// unsigned where the minimum is not negative.
    fn max(&self) -> i64 {
        if self.min >= 0 {
            self.max_unsigned
        } else {
            self.max_signed
        }
    }
}

#[derive(Debug, Default)]
struct Parser {
    globals: Globals,
    stack: Vec<Globals>,
    locals: Vec<Local>,
    usage_min: Option<RawUsage>,
    /// Next bit of each report id's input report.
    offsets: Vec<(u8, u32)>,
    reports: Vec<Report>,
}

impl Parser {
    /// Reads the item with `prefix` and the data at the start of `tail`;
    /// returns what follows.
    fn item<'a>(&mut self, prefix: u8, tail: &'a [u8]) -> Result<&'a [u8], HidError> {
        if prefix == 0xFE {
            // A long item: length, tag, data. Nothing in use has one.
            let len = usize::from(*tail.first().ok_or(HidError::Truncated)?);
            return tail.get(2 + len..).ok_or(HidError::Truncated);
        }
        let len = match prefix & 3 {
            3 => 4,
            n => usize::from(n),
        };
        let data = tail.get(..len).ok_or(HidError::Truncated)?;
        let raw = data
            .iter()
            .enumerate()
            .fold(0u32, |v, (i, b)| v | u32::from(*b) << (8 * i));
        let tag = prefix >> 4;
        match (prefix >> 2) & 3 {
            0 => self.main(tag, raw)?,
            1 => self.global(tag, raw, len)?,
            2 => self.local(tag, raw, len),
            _ => {}
        }
        Ok(&tail[len..])
    }

    fn global(&mut self, tag: u8, raw: u32, len: usize) -> Result<(), HidError> {
        let g = &mut self.globals;
        match tag {
            0x0 => g.page = raw,
            0x1 => g.min = signed(raw, len),
            0x2 => {
                g.max_signed = signed(raw, len);
                g.max_unsigned = i64::from(raw);
            }
            0x7 => g.size = raw,
            0x8 => g.id = u8::try_from(raw).map_err(|_| HidError::Unsupported)?,
            0x9 => g.count = raw,
            0xA => {
                if self.stack.len() >= MAX_PUSH {
                    return Err(HidError::TooLarge);
                }
                self.stack.push(*g);
            }
            0xB => *g = self.stack.pop().ok_or(HidError::Unsupported)?,
            _ => {}
        }
        Ok(())
    }

    fn local(&mut self, tag: u8, raw: u32, len: usize) {
        let usage = RawUsage {
            value: raw,
            extended: len == 4,
        };
        match tag {
            0x0 => self.locals.push(Local::Usage(usage)),
            0x1 => self.usage_min = Some(usage),
            0x2 => {
                if let Some(min) = self.usage_min.take() {
                    self.locals.push(Local::Range(min, usage));
                }
            }
            _ => {}
        }
    }

    fn main(&mut self, tag: u8, raw: u32) -> Result<(), HidError> {
        if tag == 0x8 {
            self.input(raw)?;
        }
        // Collections, their ends, outputs and features: the local items
        // are used up.
        self.locals.clear();
        self.usage_min = None;
        Ok(())
    }

    /// The next bit of the input report `id`, advanced by `bits`.
    fn advance(&mut self, id: u8, bits: u32) -> Result<u32, HidError> {
        let index = match self.offsets.iter().position(|(i, _)| *i == id) {
            Some(index) => index,
            None => {
                self.offsets.push((id, 0));
                self.offsets.len() - 1
            }
        };
        let start = self.offsets[index].1;
        let end = start.checked_add(bits).ok_or(HidError::TooLarge)?;
        if end > MAX_REPORT_BITS {
            return Err(HidError::TooLarge);
        }
        self.offsets[index].1 = end;
        Ok(start)
    }

    fn input(&mut self, flags: u32) -> Result<(), HidError> {
        let g = self.globals;
        let bits = g.size.checked_mul(g.count).ok_or(HidError::TooLarge)?;
        let offset = self.advance(g.id, bits)?;
        let constant = flags & 1 != 0;
        let variable = flags & 2 != 0;
        if constant || bits == 0 {
            return Ok(());
        }
        let usages = self.usages(g.page, if variable { g.count } else { 0 });
        let fields = if variable {
            variable_fields(&g, offset, &usages)?
        } else {
            array_field(&g, offset, &self.locals, usages)?
        };
        if !fields.is_empty() {
            self.record(g.id, fields, offset + bits);
        }
        Ok(())
    }

    /// The usages the local items named, ranges expanded; up to `want`
    /// of them for ranges (all, up to a limit, if 0).
    fn usages(&self, page: u32, want: u32) -> Vec<u32> {
        let cap = if want == 0 { MAX_USAGES } else { want as usize }.min(MAX_USAGES);
        let mut list = Vec::new();
        for local in &self.locals {
            match *local {
                Local::Usage(u) => list.push(u.resolve(page)),
                Local::Range(a, b) => {
                    let (a, b) = (a.resolve(page), b.resolve(page));
                    let mut u = a;
                    while u <= b && list.len() < cap {
                        list.push(u);
                        u += 1;
                    }
                }
            }
            if list.len() >= cap {
                break;
            }
        }
        list.truncate(cap);
        list
    }

    fn record(&mut self, id: u8, mut fields: Vec<Field>, end: u32) {
        match self.reports.iter_mut().find(|r| r.id == id) {
            Some(report) => {
                report.fields.append(&mut fields);
                report.bits = report.bits.max(end);
            }
            None => self.reports.push(Report {
                id,
                fields,
                bits: end,
            }),
        }
    }
}

/// The fields of a variable item: one per element that is a key.
fn variable_fields(g: &Globals, offset: u32, usages: &[u32]) -> Result<Vec<Field>, HidError> {
    let mut fields = Vec::new();
    let Some(last) = usages.last() else {
        return Ok(fields);
    };
    for i in 0..g.count {
        // The last usage serves the elements left over.
        let usage = usages.get(i as usize).unwrap_or(last);
        if let Some(key) = Key::from_usage(*usage) {
            let size = u8::try_from(g.size).map_err(|_| HidError::Unsupported)?;
            if size > 32 {
                return Err(HidError::Unsupported);
            }
            fields.push(Field::Variable {
                offset: offset + i * g.size,
                size,
                key,
            });
        }
    }
    Ok(fields)
}

/// The field of an array item, if it can give keys: it names a range of
/// a keyboard or consumer page, or a list with a usage of one.
fn array_field(
    g: &Globals,
    offset: u32,
    locals: &[Local],
    usages: Vec<u32>,
) -> Result<Vec<Field>, HidError> {
    let on_page = |usage: u32| matches!(usage >> 16, PAGE_KEYBOARD | PAGE_CONSUMER);
    let lookup = match locals {
        [Local::Range(a, b)] => {
            let (a, b) = (a.resolve(g.page), b.resolve(g.page));
            if b < a || !on_page(a) {
                return Ok(Vec::new());
            }
            Lookup::Range { first: a }
        }
        _ if usages.iter().copied().any(on_page) => Lookup::List(usages),
        _ => return Ok(Vec::new()),
    };
    let size = u8::try_from(g.size).map_err(|_| HidError::Unsupported)?;
    if size > 32 {
        return Err(HidError::Unsupported);
    }
    Ok(alloc::vec![Field::Array {
        offset,
        size,
        count: u16::try_from(g.count).map_err(|_| HidError::TooLarge)?,
        min: g.min,
        max: g.max(),
        lookup,
    }])
}

/// `raw`, `len` bytes long, as a signed number.
fn signed(raw: u32, len: usize) -> i64 {
    match len {
        1 => i64::from(raw as u8 as i8),
        2 => i64::from(raw as u16 as i16),
        _ => i64::from(raw as i32),
    }
}
