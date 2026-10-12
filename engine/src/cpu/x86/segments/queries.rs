use super::{SegmentDescriptorRecord, SegmentDescriptorTable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentQuery {
    AccessRights,
    Limit,
}

impl SegmentDescriptorRecord {
    /// query protected32 attributes without loading the segment or changing its accessed bit.
    pub fn query(self, selector: u16, cpl: u8, query: SegmentQuery) -> Option<u32> {
        if selector & !3 == 0 || cpl > 3 {
            return None;
        }
        let kind = self.descriptor_type();
        let conforming_code = self.code_data() && kind & 0xc == 0xc;
        if !conforming_code && (cpl > self.dpl() || (selector & 3) as u8 > self.dpl()) {
            return None;
        }
        if !self.code_data() {
            let valid = match query {
                SegmentQuery::AccessRights => matches!(kind, 1 | 2 | 3 | 4 | 5 | 9 | 11 | 12),
                SegmentQuery::Limit => matches!(kind, 1 | 2 | 3 | 9 | 11),
            };
            if !valid {
                return None;
            }
        }
        Some(match query {
            // bits 19:16 are architecturally undefined; this software profile returns zero.
            SegmentQuery::AccessRights => self.to_words().1 & 0x00f0_ff00,
            SegmentQuery::Limit => {
                let limit = self.raw_limit();
                if self.granularity() {
                    (limit << 12) | 0xfff
                } else {
                    limit
                }
            }
        })
    }
}

impl SegmentDescriptorTable {
    pub fn query(&self, selector: u16, cpl: u8, query: SegmentQuery) -> Option<u32> {
        self.raw_descriptor(selector)?.query(selector, cpl, query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(kind: u32, code_data: bool, dpl: u8, present: bool) -> SegmentDescriptorRecord {
        let access =
            kind | (u32::from(code_data) << 4) | (u32::from(dpl) << 5) | (u32::from(present) << 7);
        SegmentDescriptorRecord::from_words(0xcafe_2345, 0xabf1_0078 | (access << 8))
    }

    #[test]
    fn protected32_system_type_tables_and_every_code_data_type_are_exact() {
        let lar = [
            false, true, true, true, true, true, false, false, false, true, false, true, true,
            false, false, false,
        ];
        let lsl = [
            false, true, true, true, false, false, false, false, false, true, false, true, false,
            false, false, false,
        ];
        for kind in 0..16 {
            for present in [false, true] {
                for code_data in [false, true] {
                    let record = descriptor(kind, code_data, 3, present);
                    assert_eq!(
                        record.query(0x43, 3, SegmentQuery::AccessRights).is_some(),
                        code_data || lar[kind as usize]
                    );
                    assert_eq!(
                        record.query(0x43, 3, SegmentQuery::Limit).is_some(),
                        code_data || lsl[kind as usize]
                    );
                }
            }
        }
    }

    #[test]
    fn visibility_uses_cpl_and_rpl_with_conforming_code_bypass_even_execute_only() {
        for code_data in [false, true] {
            for kind in 0..16 {
                for dpl in 0..4 {
                    for cpl in 0..4 {
                        for rpl in 0..4 {
                            let record = descriptor(kind, code_data, dpl, false);
                            let visible = (code_data && kind >= 12) || (cpl <= dpl && rpl <= dpl);
                            for (query, system_types) in [
                                (SegmentQuery::AccessRights, &[1, 2, 3, 4, 5, 9, 11, 12][..]),
                                (SegmentQuery::Limit, &[1, 2, 3, 9, 11][..]),
                            ] {
                                let eligible = code_data || system_types.contains(&kind);
                                assert_eq!(
                                    record.query(0x40 | u16::from(rpl), cpl, query).is_some(),
                                    eligible && visible
                                );
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(
            descriptor(12, true, 0, false).query(0x43, 3, SegmentQuery::AccessRights),
            Some(0x00f0_1c00)
        );
    }

    #[test]
    fn access_rights_return_raw_defined_bits_and_choose_zero_for_undefined_bits() {
        let raw = SegmentDescriptorRecord::from_words(0xffff_ffff, 0xffff_ffff);
        assert_eq!(
            raw.query(0x43, 3, SegmentQuery::AccessRights),
            Some(0x00f0_ff00)
        );
        for bit in [
            0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 24, 25, 26, 27, 28, 29, 30, 31,
        ] {
            let high = 0x0000_f200 | (1_u32 << bit);
            assert_eq!(
                SegmentDescriptorRecord::from_words(0, high).query(
                    0x43,
                    3,
                    SegmentQuery::AccessRights
                ),
                Some(high & 0x00f0_ff00)
            );
        }
    }

    #[test]
    fn limit_scaling_precedes_operand_size_truncation_and_handles_maximum() {
        for (low, high, limit) in [
            (0x2345, 0x0001_f200, 0x12345),
            (0x2345, 0x0081_f200, 0x1234_5fff),
            (0xffff, 0x008f_f200, u32::MAX),
            (0, 0x0080_f200, 0xfff),
        ] {
            let result =
                SegmentDescriptorRecord::from_words(low, high).query(0x43, 3, SegmentQuery::Limit);
            assert_eq!(result, Some(limit));
            assert_eq!(result.unwrap() as u16, limit as u16);
        }
    }

    #[test]
    fn null_missing_and_invalid_cpl_fail_without_changing_raw_table_or_loaded_authority() {
        let mut table = SegmentDescriptorTable::default();
        let raw = descriptor(2, true, 3, false);
        table.configure_raw(0x43, raw).unwrap();
        table.configure_raw(4, raw).unwrap();
        let before = table.clone();
        for query in [SegmentQuery::AccessRights, SegmentQuery::Limit] {
            for selector in [0, 1, 2, 3, 0x53, 0xffff] {
                assert_eq!(table.query(selector, 3, query), None);
            }
            for rpl in 0..4 {
                assert!(table.query(4 | rpl, 3, query).is_some());
            }
            assert_eq!(table.query(0x43, 4, query), None);
            assert!(table.query(0x43, 3, query).is_some());
        }
        assert_eq!(table, before);
        assert_eq!(table.raw_descriptor(0x43), Some(&raw));
        assert_eq!(raw.descriptor_type(), 2);
    }
}
