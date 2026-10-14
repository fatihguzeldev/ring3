use super::{SegmentCache, SegmentDescriptor, SegmentDescriptorKind};

/// exact eight-byte descriptor; system and gate payloads are never reconstructed as code/data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentDescriptorRecord([u8; 8]);

impl SegmentDescriptorRecord {
    pub const fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    pub const fn to_bytes(self) -> [u8; 8] {
        self.0
    }

    pub const fn from_words(low: u32, high: u32) -> Self {
        let low = low.to_le_bytes();
        let high = high.to_le_bytes();
        Self([
            low[0], low[1], low[2], low[3], high[0], high[1], high[2], high[3],
        ])
    }

    pub const fn to_words(self) -> (u32, u32) {
        (
            u32::from_le_bytes([self.0[0], self.0[1], self.0[2], self.0[3]]),
            u32::from_le_bytes([self.0[4], self.0[5], self.0[6], self.0[7]]),
        )
    }

    /// legacy code/data construction is explicitly flat32 with the accessed bit already set.
    /// legacy system chooses reserved type zero; it does not imply a tss or a gate.
    pub fn from_legacy(descriptor: SegmentDescriptor) -> Option<Self> {
        if descriptor.dpl > 3 {
            return None;
        }
        let (kind, attributes, limit) = match descriptor.kind {
            SegmentDescriptorKind::Data { writable } => {
                (0x11 | (u8::from(writable) << 1), 0x00cf_0000, 0xffff)
            }
            SegmentDescriptorKind::Code {
                readable,
                conforming,
            } => (
                0x19 | (u8::from(readable) << 1) | (u8::from(conforming) << 2),
                0x00cf_0000,
                0xffff,
            ),
            SegmentDescriptorKind::System => (0, 0, 0),
        };
        let access = u32::from(kind)
            | (u32::from(descriptor.dpl) << 5)
            | (u32::from(descriptor.present) << 7);
        Some(Self::from_words(
            (descriptor.base << 16) | limit,
            (descriptor.base & 0xff00_0000)
                | ((descriptor.base >> 16) & 0xff)
                | (access << 8)
                | attributes,
        ))
    }

    pub const fn descriptor_type(self) -> u8 {
        self.0[5] & 0xf
    }
    pub const fn code_data(self) -> bool {
        self.0[5] & 0x10 != 0
    }
    pub const fn dpl(self) -> u8 {
        (self.0[5] >> 5) & 3
    }
    pub const fn present(self) -> bool {
        self.0[5] & 0x80 != 0
    }
    /// these bits are a segment limit only for descriptors whose type defines that field.
    pub const fn raw_limit(self) -> u32 {
        u16::from_le_bytes([self.0[0], self.0[1]]) as u32 | (((self.0[6] & 0xf) as u32) << 16)
    }
    pub const fn granularity(self) -> bool {
        self.0[6] & 0x80 != 0
    }
    pub const fn default32(self) -> bool {
        self.0[6] & 0x40 != 0
    }
    pub const fn long_mode(self) -> bool {
        self.0[6] & 0x20 != 0
    }
    pub const fn available(self) -> bool {
        self.0[6] & 0x10 != 0
    }
    pub const fn code_data_base(self) -> Option<u32> {
        if self.code_data() {
            Some(u32::from_le_bytes([
                self.0[2], self.0[3], self.0[4], self.0[7],
            ]))
        } else {
            None
        }
    }

    pub fn with_code_data_base(mut self, base: u32) -> Option<Self> {
        if !self.code_data() {
            return None;
        }
        let base = base.to_le_bytes();
        self.0[2] = base[0];
        self.0[3] = base[1];
        self.0[4] = base[2];
        self.0[7] = base[3];
        Some(self)
    }

    /// task return clears only the old live descriptor's busy bit, without rebuilding it.
    pub(crate) fn with_task_busy_cleared(mut self) -> Self {
        self.0[5] &= !2;
        self
    }

    pub fn with_accessed(mut self) -> Option<Self> {
        if !self.code_data() {
            return None;
        }
        self.0[5] |= 1;
        Some(self)
    }

    pub(super) fn legacy_code_data(self, selector: u16) -> Option<SegmentDescriptor> {
        let base = self.code_data_base()?;
        let kind = self.descriptor_type();
        Some(SegmentDescriptor {
            selector,
            base,
            dpl: self.dpl(),
            present: self.present(),
            kind: if kind & 8 != 0 {
                SegmentDescriptorKind::Code {
                    readable: kind & 2 != 0,
                    conforming: kind & 4 != 0,
                }
            } else {
                SegmentDescriptorKind::Data {
                    writable: kind & 2 != 0,
                }
            },
        })
    }

    /// successful loads cache the raw code/data attributes and set accessed in that snapshot.
    pub(in crate::cpu::x86) fn loaded_cache(self) -> Option<SegmentCache> {
        if !self.code_data() || !self.present() || self.long_mode() {
            return None;
        }
        SegmentCache::from_bits(
            self.raw_limit()
                | (u32::from(self.descriptor_type() | 1) << 20)
                | (u32::from(self.granularity()) << 24)
                | (u32::from(self.default32()) << 25)
                | (u32::from(self.available()) << 27)
                | (1 << 28)
                | (u32::from(self.dpl()) << 29)
                | (1 << 31),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_raw_bit_survives_words_and_bytes_without_system_projection() {
        assert_eq!(std::mem::size_of::<SegmentDescriptorRecord>(), 8);
        for bit in 0..64 {
            let raw = 1_u64 << bit;
            let record = SegmentDescriptorRecord::from_words(raw as u32, (raw >> 32) as u32);
            assert_eq!(record.to_bytes(), raw.to_le_bytes());
            assert_eq!(
                SegmentDescriptorRecord::from_bytes(record.to_bytes()),
                record
            );
            assert_eq!(record.to_words(), (raw as u32, (raw >> 32) as u32));
        }
        for kind in 0..16 {
            let raw = SegmentDescriptorRecord::from_words(0x1234_5678, 0x9abc_e0de | (kind << 8));
            assert_eq!(raw.descriptor_type(), kind as u8);
            assert!(!raw.code_data());
            assert_eq!(raw.code_data_base(), None);
            assert_eq!(raw.legacy_code_data(0x40), None);
            assert_eq!(raw.with_code_data_base(0), None);
            assert_eq!(raw.with_accessed(), None);
            assert_eq!(raw.to_words(), (0x1234_5678, 0x9abc_e0de | (kind << 8)));
        }
    }

    #[test]
    fn raw_fields_and_mutations_preserve_unrelated_attributes() {
        let raw = SegmentDescriptorRecord::from_words(0x1234_5678, 0x9abc_def0);
        assert_eq!(raw.code_data_base(), Some(0x9af0_1234));
        assert_eq!(
            (raw.descriptor_type(), raw.dpl(), raw.present()),
            (14, 2, true)
        );
        assert_eq!(
            (
                raw.raw_limit(),
                raw.granularity(),
                raw.default32(),
                raw.long_mode(),
                raw.available()
            ),
            (0xc5678, true, false, true, true)
        );
        assert_eq!(
            raw.with_accessed().unwrap().to_words(),
            (0x1234_5678, 0x9abc_dff0)
        );
        assert_eq!(
            raw.with_code_data_base(0x1122_3344).unwrap().to_words(),
            (0x3344_5678, 0x11bc_de22)
        );
        assert_eq!(raw.loaded_cache(), None);
    }

    #[test]
    fn legacy_constructors_keep_explicit_flat_and_reserved_system_policies() {
        for kind in [
            SegmentDescriptorKind::Data { writable: false },
            SegmentDescriptorKind::Data { writable: true },
            SegmentDescriptorKind::Code {
                readable: false,
                conforming: false,
            },
            SegmentDescriptorKind::Code {
                readable: true,
                conforming: false,
            },
            SegmentDescriptorKind::Code {
                readable: false,
                conforming: true,
            },
            SegmentDescriptorKind::Code {
                readable: true,
                conforming: true,
            },
            SegmentDescriptorKind::System,
        ] {
            for dpl in 0..4 {
                for present in [false, true] {
                    let descriptor = SegmentDescriptor {
                        selector: 0x40,
                        base: 0x89ab_cdef,
                        dpl,
                        present,
                        kind,
                    };
                    let raw = SegmentDescriptorRecord::from_legacy(descriptor).unwrap();
                    assert_eq!((raw.dpl(), raw.present()), (dpl, present));
                    if kind == SegmentDescriptorKind::System {
                        assert_eq!(
                            raw.to_words(),
                            (
                                0xcdef_0000,
                                0x8900_00ab | (u32::from(dpl) << 13) | (u32::from(present) << 15)
                            )
                        );
                        assert_eq!(raw.code_data_base(), None);
                    } else {
                        assert_eq!(raw.legacy_code_data(0x40), Some(descriptor));
                        assert_eq!(raw.raw_limit(), 0xfffff);
                        assert!(raw.granularity() && raw.default32());
                        assert!(!raw.long_mode() && !raw.available());
                        assert_ne!(raw.descriptor_type() & 1, 0);
                    }
                }
            }
        }
        assert_eq!(
            SegmentDescriptorRecord::from_legacy(SegmentDescriptor {
                selector: 0x40,
                base: 0,
                dpl: 4,
                present: true,
                kind: SegmentDescriptorKind::System
            }),
            None
        );
    }

    #[test]
    fn loaded_cache_uses_exact_raw_limit_type_and_attributes() {
        let raw = SegmentDescriptorRecord::from_words(0x5678_1234, 0x9a50_f6bc);
        assert_eq!(raw.code_data_base(), Some(0x9abc_5678));
        assert_eq!(raw.loaded_cache().unwrap().bits(), 0xfa70_1234);
        assert_eq!(raw.loaded_cache().unwrap().limit(), 0x1234);
        let paged = SegmentDescriptorRecord::from_words(0x5678_1234, 0x9ad5_f6bc);
        assert_eq!(paged.loaded_cache().unwrap().bits(), 0xfb75_1234);
        assert_eq!(paged.loaded_cache().unwrap().limit(), 0x5123_4fff);
        assert_eq!(
            SegmentDescriptorRecord::from_words(0, 0x00cf_7300).loaded_cache(),
            None
        );
    }
}
