//! Record layout for the little-endian C ABIs supported by Aura.
use super::{CField, CStruct};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Default, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LayoutControls {
    pub packed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inline_union: bool,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub inline_arrays: std::collections::BTreeSet<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub microsoft_bit_fields: bool,
    pub alignment: Option<usize>,
    pub bit_widths: BTreeMap<String, u8>,
}

impl LayoutControls {
    pub fn is_default(&self) -> bool {
        !self.inline_union
            && self.inline_arrays.is_empty()
            && !self.packed
            && self.alignment.is_none()
            && self.bit_widths.is_empty()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Position {
    pub byte: usize,
    pub bits: Option<(u8, u8)>,
    end: usize,
}

#[derive(Default)]
struct Cursor {
    end_bits: usize,
    // MSVC reserves a full allocation unit and never coalesces different
    // integral base widths. Keep its next free bit across adjacent members.
    msvc_unit: Option<(usize, usize)>,
}

impl CStruct {
    pub(crate) fn positions(&self) -> impl Iterator<Item = (&CField, Position)> {
        self.positions_for(self.layout_controls.microsoft_bit_fields)
    }

    fn positions_for(&self, microsoft: bool) -> impl Iterator<Item = (&CField, Position)> {
        let mut cursor = Cursor::default();
        self.fields.iter().map(move |field| {
            let width = self.layout_controls.bit_widths.get(&field.name).copied();
            let unit = field.kind.size() * 8;
            let align = if self.layout_controls.packed {
                8
            } else {
                field.kind.align() * 8
            };
            let bit = if self.union {
                0
            } else if let Some(n) = width {
                let n = usize::from(n);
                if microsoft {
                    let bit = match cursor.msvc_unit {
                        Some((last, next)) if last == unit && next + n <= cursor.end_bits => next,
                        _ => {
                            let start = cursor.end_bits.next_multiple_of(align);
                            cursor.end_bits = start + unit;
                            start
                        }
                    };
                    cursor.msvc_unit = Some((unit, bit + n));
                    bit
                } else {
                    let mut start = cursor.end_bits;
                    if !self.layout_controls.packed && start % unit + n > unit {
                        start = start.next_multiple_of(unit);
                    }
                    cursor.end_bits = start + n;
                    start
                }
            } else {
                cursor.msvc_unit = None;
                let start = cursor.end_bits.next_multiple_of(align);
                cursor.end_bits = start + unit;
                start
            };
            (
                field,
                Position {
                    byte: bit / 8,
                    end: if self.union {
                        field.kind.size()
                    } else {
                        cursor.end_bits.div_ceil(8)
                    },
                    bits: width.map(|n| ((bit % 8) as u8, n)),
                },
            )
        })
    }

    pub(super) fn controlled_size(&self) -> usize {
        let end = self
            .positions()
            .map(|(_, position)| position.end)
            .max()
            .unwrap_or(0);
        end.next_multiple_of(self.align())
    }
}

/// The bits a field occupies in each successive byte, starting at its
/// first occupied byte. Each tuple gives the field shift, byte shift and
/// the mask of the bits occupied in that byte.
pub fn bit_chunks(start: u8, width: u8) -> impl Iterator<Item = (u8, u8, u8)> {
    (0..(start + width).div_ceil(8)).map(move |index| {
        let first = start.max(index * 8);
        let end = (start + width).min((index + 1) * 8);
        let shift = first % 8;
        let mask = (u8::MAX >> (8 - (end - first))) << shift;
        (first - start, shift, mask)
    })
}

#[derive(Clone, Copy)]
pub enum StoragePart {
    Scalar(super::CLeaf),
    Inline { offset: usize },
}

impl CStruct {
    /// Native values carry each scalar as a word and each inline array as
    /// one aggregate. C unions in freestanding code carry one byte array.
    pub fn storage_parts(&self) -> Vec<StoragePart> {
        fn visit(record: &CStruct, base: usize, parts: &mut Vec<StoragePart>) {
            use super::{CFieldKind, CLeaf};
            if record.layout_controls.inline_union {
                parts.push(StoragePart::Inline { offset: base });
                return;
            }
            for (field, position) in record.positions() {
                let offset = base + position.byte;
                match &field.kind {
                    CFieldKind::Scalar(scalar) => parts.push(StoragePart::Scalar(CLeaf {
                        offset,
                        scalar: *scalar,
                        bits: position.bits,
                    })),
                    CFieldKind::Struct(inner) => visit(inner, offset, parts),
                    CFieldKind::Array(_, _) => parts.push(StoragePart::Inline { offset }),
                }
            }
        }
        let mut parts = vec![];
        visit(self, 0, &mut parts);
        parts
    }
}
