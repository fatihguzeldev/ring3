use super::*;
use crate::cpu::x86::tasks::encode_return;

fn save() -> TaskSave {
    TaskSave {
        next_eip: 0x1234_5678,
        flags: 0x254fd7,
        general: [
            0x1122_3344,
            0x5566_7788,
            0x99aa_bbcc,
            0xddee_ff00,
            0x1357_9bdf,
            0x2468_ace0,
            0xdead_beef,
            0xcafe_babe,
        ],
        selectors: [0x23, 0x1b, 0x2b, 0x33, 0x3b, 0x43],
    }
}

fn literal_words(format: TaskFormat) -> Vec<(u16, u16)> {
    match format {
        TaskFormat::Word => vec![
            (14, 0x5678),
            (16, 0x0fd7),
            (18, 0x3344),
            (20, 0x7788),
            (22, 0xbbcc),
            (24, 0xff00),
            (26, 0x9bdf),
            (28, 0xace0),
            (30, 0xbeef),
            (32, 0xbabe),
            (34, 0x23),
            (36, 0x1b),
            (38, 0x2b),
            (40, 0x33),
        ],
        TaskFormat::Dword => vec![
            (32, 0x5678),
            (34, 0x1234),
            (36, 0x0fd7),
            (38, 0x25),
            (40, 0x3344),
            (42, 0x1122),
            (44, 0x7788),
            (46, 0x5566),
            (48, 0xbbcc),
            (50, 0x99aa),
            (52, 0xff00),
            (54, 0xddee),
            (56, 0x9bdf),
            (58, 0x1357),
            (60, 0xace0),
            (62, 0x2468),
            (64, 0xbeef),
            (66, 0xdead),
            (68, 0xbabe),
            (70, 0xcafe),
            (72, 0x23),
            (76, 0x1b),
            (80, 0x2b),
            (84, 0x33),
            (88, 0x3b),
            (92, 0x43),
        ],
    }
}

#[test]
fn exact_fourteen_and_twenty_six_word_plans_clear_only_saved_nt() {
    for (format, count, limit) in [(TaskFormat::Word, 14, 41), (TaskFormat::Dword, 26, 93)] {
        let plan = TaskSavePlan::new(format, &save());
        assert_eq!(
            plan.words()
                .iter()
                .map(|word| (word.offset, word.value))
                .collect::<Vec<_>>(),
            literal_words(format)
        );
        assert_eq!(plan.words().len(), count);
        assert_eq!(plan.required_limit(), limit);
        assert!(
            plan.words()
                .windows(2)
                .all(|pair| pair[0].offset < pair[1].offset)
        );
    }
}

#[test]
fn plan_matches_existing_encoder_without_publishing_static_reserved_or_tail_bytes() {
    for format in [TaskFormat::Word, TaskFormat::Dword] {
        let original: [u8; 128] = std::array::from_fn(|index| index as u8 ^ 0xa5);
        let mut encoded = original;
        encode_return(format, &save(), &mut encoded).unwrap();
        let mut planned = original;
        for (offset, value) in literal_words(format) {
            planned[usize::from(offset)..usize::from(offset) + 2]
                .copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(encoded, planned);
        let mut overlay = original;
        TaskSavePlan::new(format, &save())
            .overlay(
                0x5000,
                0x5000,
                &mut overlay,
                TaskSaveOverlayAuthority::SameLinearAddressSpace,
            )
            .unwrap();
        assert_eq!(overlay, planned);
        assert_eq!(&overlay[..14], &original[..14]);
        assert_eq!(&overlay[104..], &original[104..]);
    }
}

#[test]
fn linear_overlay_handles_self_partial_and_wrapped_images_without_touching_unrelated_bytes() {
    for format in [TaskFormat::Word, TaskFormat::Dword] {
        let plan = TaskSavePlan::new(format, &save());
        let mut full = [0xa5; 128];
        for (offset, value) in literal_words(format) {
            full[usize::from(offset)..usize::from(offset) + 2]
                .copy_from_slice(&value.to_le_bytes());
        }
        let mut partial = [0xa5; 64];
        plan.overlay(
            0x5000,
            0x5021,
            &mut partial,
            TaskSaveOverlayAuthority::SameLinearAddressSpace,
        )
        .unwrap();
        assert_eq!(partial, full[33..97]);
        let mut unrelated = [0xa5; 128];
        plan.overlay(
            0x5000,
            0x6000,
            &mut unrelated,
            TaskSaveOverlayAuthority::SameLinearAddressSpace,
        )
        .unwrap();
        assert_eq!(unrelated, [0xa5; 128]);
        let (outgoing, expected_prefix): (u32, &[u8]) = match format {
            TaskFormat::Word => (0xffff_fff1, &[0x56, 0xd7, 0x0f, 0x44, 0x33, 0x88, 0x77]),
            TaskFormat::Dword => (0xffff_ffdf, &[0x56, 0x34, 0x12, 0xd7, 0x0f, 0x25, 0]),
        };
        let mut low = [0xa5; 96];
        plan.overlay(
            outgoing,
            0,
            &mut low,
            TaskSaveOverlayAuthority::SameLinearAddressSpace,
        )
        .unwrap();
        assert_eq!(&low[..expected_prefix.len()], expected_prefix);
        let mut expected = [0xa5; 96];
        for (offset, value) in literal_words(format) {
            let address = outgoing.wrapping_add(u32::from(offset));
            for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
                let destination = address.wrapping_add(index as u32);
                if destination < 96 {
                    expected[destination as usize] = byte;
                }
            }
        }
        assert_eq!(low, expected);
        let mut high = [0xa5; 32];
        plan.overlay(
            outgoing,
            0xffff_ffe0,
            &mut high,
            TaskSaveOverlayAuthority::SameLinearAddressSpace,
        )
        .unwrap();
        assert_eq!(high[31], 0x78);
        assert_eq!(&high[..31], &[0xa5; 31]);
    }
}

#[test]
fn unidentified_physical_aliases_are_rejected_before_any_staged_image_change() {
    for format in [TaskFormat::Word, TaskFormat::Dword] {
        let mut incoming = [0x7b; 128];
        assert_eq!(
            TaskSavePlan::new(format, &save()).overlay(
                0x5000,
                0x5000,
                &mut incoming,
                TaskSaveOverlayAuthority::UnidentifiedPhysicalAliases
            ),
            Err(TaskSaveOverlayError::UnidentifiedPhysicalAliases)
        );
        assert_eq!(incoming, [0x7b; 128]);
    }
}
