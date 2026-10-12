use ring3_engine::{
    cpu::x86::{
        decode::decode_one,
        ir::Operation,
        packed_strings::{
            PackedStringCompare, PackedStringLength, PackedStringOutput, PackedStringSource,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

fn decode(bytes: &[u8]) -> PackedStringCompare {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let instruction = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert_eq!(usize::from(instruction.length()), bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(0x1000 + bytes.len() as u32)
    );
    let Operation::PackedStringCompare(operation) = instruction.operation() else {
        panic!("dedicated packed-string operation expected")
    };
    assert!(operation.valid());
    *operation
}

#[test]
fn packed_strings_decode_all_four_forms_and_every_control() {
    for opcode in 0x60..=0x63 {
        for control in 0..=u8::MAX {
            let op = decode(&[0x66, 0x0f, 0x3a, opcode, 0xc1, control]);
            assert_eq!(op.control, control);
            assert_eq!(
                op.length,
                if opcode < 0x62 {
                    PackedStringLength::Explicit
                } else {
                    PackedStringLength::NullTerminated
                }
            );
            assert_eq!(
                op.output,
                if opcode & 1 == 0 {
                    PackedStringOutput::Mask
                } else {
                    PackedStringOutput::Index
                }
            );
        }
    }
}

#[test]
fn packed_strings_decode_every_xmm_input_and_output_alias() {
    for opcode in 0x60..=0x63 {
        for left in 0..8 {
            for right in 0..8 {
                let op = decode(&[0x66, 0x0f, 0x3a, opcode, 0xc0 | left << 3 | right, 0xff]);
                assert_eq!(op.left, left);
                assert_eq!(op.right, PackedStringSource::Xmm(right));
            }
        }
    }
}

fn words(values: [u16; 8]) -> [u8; 16] {
    let mut out = [0; 16];
    for (index, value) in values.into_iter().enumerate() {
        out[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    out
}

#[test]
fn packed_strings_all_aggregation_polarity_and_signed_widths_have_literal_results() {
    for format in 0..4u8 {
        let (left, right, full_mask) = if format & 1 == 0 {
            (
                [0x80, 0x7f, 0x20, 0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [
                    0x7f, 0x80, 0x20, 0x30, 0x25, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
                0xffff,
            )
        } else {
            (
                words([0x8000, 0x7fff, 0x20, 0x30, 0, 0, 0, 0]),
                words([0x7fff, 0x8000, 0x20, 0x30, 0x25, 0xffff, 0, 0]),
                0xff,
            )
        };
        for aggregation in 0..4u8 {
            let positive = match aggregation {
                0 => 0xf,
                1 => {
                    if format & 2 == 0 {
                        0x1c
                    } else {
                        0x3f
                    }
                }
                2 => (full_mask & !0x3f) | 0xc,
                _ => 0,
            };
            for polarity in 0..4u8 {
                let control = format | aggregation << 2 | polarity << 4;
                let bits = match polarity {
                    1 => positive ^ full_mask,
                    3 => positive ^ 0x3f,
                    _ => positive,
                };
                for opcode in 0x60..=0x63 {
                    let op = decode(&[0x66, 0x0f, 0x3a, opcode, 0xc1, control]);
                    let result = op.compare(left, right, 4, 6, 0xcd7);
                    assert_eq!(result.bits, bits, "opcode={opcode:x} control={control:x}");
                    assert_eq!(
                        result.flags,
                        0x4c2 | u32::from(bits != 0) | (u32::from(bits & 1 != 0) << 11)
                    );
                }
            }
        }
    }
}

#[test]
fn packed_strings_explicit_lengths_saturate_absolute_values_and_keep_embedded_zero_valid() {
    let op = decode(&[0x66, 0x0f, 0x3a, 0x61, 0xc1, 0]);
    for length in [16, 17, i32::MAX as u32, 0x8000_0000, (-16i32) as u32] {
        let result = op.compare([7; 16], [7; 16], length, length, 0xcd7);
        assert_eq!(result.bits, 0xffff);
        assert_eq!(result.flags, 0xc03);
    }
    let mut data = [0; 16];
    data[1] = 0x2a;
    assert_eq!(op.compare(data, data, (-2i32) as u32, 2, 0).bits, 3);
    let implicit = decode(&[0x66, 0x0f, 0x3a, 0x63, 0xc1, 0]);
    let result = implicit.compare(data, data, 16, 16, 0xcd7);
    assert_eq!(result.bits, 0);
    assert_eq!(result.flags, 0x4c2);
}

#[test]
fn packed_strings_ordered_boundary_and_invalid_polarity_match_sdm_tables() {
    let mut left = [9; 16];
    left[..4].copy_from_slice(&[1, 2, 3, 4]);
    let mut right = [9; 16];
    right[14..].copy_from_slice(&[1, 2]);
    let op = decode(&[0x66, 0x0f, 0x3a, 0x61, 0xc1, 0xc]);
    assert_eq!(op.compare(left, right, 4, 16, 0).bits, 0x4000);
    for (control, expected) in [(0xc, 0xffff), (0x1c, 0), (0x2c, 0xffff), (0x3c, 0xfffe)] {
        let op = decode(&[0x66, 0x0f, 0x3a, 0x61, 0xc1, control]);
        assert_eq!(op.compare(left, right, 0, 1, 0).bits, expected);
    }
    let mut ranges = [0; 16];
    ranges[..3].copy_from_slice(&[10, 20, 30]);
    let mut values = [0; 16];
    values[..4].copy_from_slice(&[10, 20, 30, 40]);
    let op = decode(&[0x66, 0x0f, 0x3a, 0x61, 0xc1, 4]);
    assert_eq!(op.compare(ranges, values, 3, 4, 0).bits, 3);
}

#[test]
fn packed_strings_index_mask_and_reserved_control_bits_preserve_raw_data() {
    let op = decode(&[0x66, 0x0f, 0x3a, 0x61, 0xc1, 0]);
    let mut left = [0; 16];
    left[0] = 0xff;
    let mut right = [0; 16];
    right[3] = 0xff;
    right[15] = 0xff;
    let result = op.compare(left, right, 1, 16, 0xcd7);
    assert_eq!(result.bits, 0x8008);
    assert_eq!(result.index(0), 3);
    assert_eq!(result.index(0x40), 15);
    assert_eq!(
        result.mask(0),
        [8, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(result.mask(0x40), right);
    let reserved = decode(&[0x66, 0x0f, 0x3a, 0x61, 0xc1, 0x80]);
    assert_eq!(reserved.compare(left, right, 1, 16, 0xcd7), result);
    assert!(!PackedStringCompare { left: 8, ..op }.valid());
    assert!(
        !PackedStringCompare {
            right: PackedStringSource::Xmm(8),
            ..op
        }
        .valid()
    );
}

#[test]
fn packed_strings_decode_unaligned_memory_and_address_prefixes_without_reading_source() {
    for opcode in 0x60..=0x63 {
        // only the code page exists: decoding must not fetch the data operand.
        decode(&[0x66, 0x0f, 0x3a, opcode, 0x05, 0xf1, 0xff, 0xff, 0x7f, 0]);
        decode(&[0x66, 0x0f, 0x3a, opcode, 0x04, 0x89, 0xff]);
        decode(&[0x64, 0x66, 0x0f, 0x3a, opcode, 0x01, 0]);
        decode(&[0x67, 0x66, 0x0f, 0x3a, opcode, 0x00, 0xff]);
    }
}

#[test]
fn packed_strings_all_bound_profiles_admit_register_and_memory_without_eager_reads() {
    for opcode in 0x60..=0x63 {
        for modrm in [0xc1, 0x07] {
            for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)]
            {
                let mut engine = EngineInstance::new(8, 7).unwrap();
                engine.map(0x1000, 1, 7).unwrap();
                let code = [0x66, 0x0f, 0x3a, opcode, modrm, 0, 0xeb, 0xfe];
                engine.arena_mut().unwrap()[140..148].copy_from_slice(&code);
                engine.upload(0x1000, code.len() as u32).unwrap();
                engine.protect(0x1000, 1, 5).unwrap();
                engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000u32.to_le_bytes());
                if !entries {
                    engine.arena_mut().unwrap()[144..148]
                        .copy_from_slice(&(code.len() as u32).to_le_bytes());
                }
                let result = match (resident, entries) {
                    (false, false) => engine.compile(1).map(|_| ()),
                    (false, true) => engine.compile_entries(1, 0).map(|_| ()),
                    (true, false) => engine.compile_resident(1).map(|_| ()),
                    (true, true) => engine.compile_resident_entries(1, 0).map(|_| ()),
                };
                assert_eq!(
                    result,
                    Ok(()),
                    "opcode={opcode:x} modrm={modrm:x} resident={resident} entries={entries}"
                );
            }
        }
    }
}
