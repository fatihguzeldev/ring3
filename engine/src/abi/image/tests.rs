use super::*;
use crate::memory::GuestAddress;

const IMAGE: ImageMetadata32 = ImageMetadata32 {
    image_base: 0x4433_2211,
    image_size: 0x8877_6655,
    entry_point: 0xccbb_aa99,
    mapped_pages: 0x0403_0201,
};
const GATE_BASE: u32 = 0x1020_3040;
const GATES: [GateSpec; 8] = [
    GateSpec {
        entry: GuestAddress(0x1122_0000),
        id: 0x0102_0304,
    },
    GateSpec {
        entry: GuestAddress(0x1122_0010),
        id: 0x1112_1314,
    },
    GateSpec {
        entry: GuestAddress(0x1122_0020),
        id: 0x2122_2324,
    },
    GateSpec {
        entry: GuestAddress(0x1122_0030),
        id: 0x3132_3334,
    },
    GateSpec {
        entry: GuestAddress(0x1122_0040),
        id: 0x4142_4344,
    },
    GateSpec {
        entry: GuestAddress(0x1122_0050),
        id: 0x5152_5354,
    },
    GateSpec {
        entry: GuestAddress(0x1122_0060),
        id: 0x6162_6364,
    },
    GateSpec {
        entry: GuestAddress(0x1122_0070),
        id: 0x7172_7374,
    },
];
const LINKED_PAYLOAD: [u8; 96] = [
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 1, 2, 3, 4, 0x40, 0x30,
    0x20, 0x10, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x22, 0x11, 4, 3, 2, 1, 0x10, 0, 0x22,
    0x11, 0x14, 0x13, 0x12, 0x11, 0x20, 0, 0x22, 0x11, 0x24, 0x23, 0x22, 0x21, 0x30, 0, 0x22, 0x11,
    0x34, 0x33, 0x32, 0x31, 0x40, 0, 0x22, 0x11, 0x44, 0x43, 0x42, 0x41, 0x50, 0, 0x22, 0x11, 0x54,
    0x53, 0x52, 0x51, 0x60, 0, 0x22, 0x11, 0x64, 0x63, 0x62, 0x61, 0x70, 0, 0x22, 0x11, 0x74, 0x73,
    0x72, 0x71,
];

#[test]
fn image_record_matches_literal_wire_bytes_and_keeps_transfer_tail() {
    let mut output = [0xa5; 40];
    encode_image(&IMAGE, &mut output[..32]).unwrap();
    assert_eq!(
        &output[..32],
        &[
            b'R', b'3', b'P', b'E', 1, 0, 1, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0x11, 0x22, 0x33, 0x44,
            0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 1, 2, 3, 4,
        ]
    );
    assert_eq!(&output[32..], &[0xa5; 8]);
    assert_atomic_lengths(32, |output| encode_image(&IMAGE, output));
}

fn assert_atomic_lengths(size: usize, encode: impl Fn(&mut [u8]) -> Result<(), AbiError>) {
    for length in [0, size - 1, size + 1] {
        let mut output = vec![0xa5; length];
        let before = output.clone();
        assert_eq!(encode(&mut output), Err(AbiError::Length));
        assert_eq!(output, before);
    }
}

fn assert_linked_record(header: [u8; 16], encode: impl Fn(&mut [u8]) -> Result<(), AbiError>) {
    let size = usize::from(header[8]);
    let mut output = [0xa5; 120];
    encode(&mut output[..size]).unwrap();
    assert_eq!(&output[..16], &header);
    assert_eq!(&output[16..size], &LINKED_PAYLOAD[..size - 16]);
    assert!(output[size..].iter().all(|byte| *byte == 0xa5));
    assert_atomic_lengths(size, encode);
}

#[test]
fn every_linked_image_version_keeps_its_exact_header_gate_slots_and_reserved_bytes() {
    assert_linked_record(
        [b'R', b'3', b'L', b'I', 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0],
        |output| {
            encode_linked_image_v1(
                &LinkedImageMetadata32 {
                    image: IMAGE,
                    gate_base: GATE_BASE,
                    gate_count: 2,
                    gates: GATES[..2].try_into().unwrap(),
                },
                output,
            )
        },
    );
    assert_linked_record(
        [b'R', b'3', b'L', b'I', 2, 0, 1, 0, 72, 0, 0, 0, 0, 0, 0, 0],
        |output| {
            encode_linked_image_v2(
                &LinkedImageMetadata32V2 {
                    image: IMAGE,
                    gate_base: GATE_BASE,
                    gate_count: 2,
                    gates: GATES[..3].try_into().unwrap(),
                },
                output,
            )
        },
    );
    assert_linked_record(
        [b'R', b'3', b'L', b'I', 3, 0, 1, 0, 80, 0, 0, 0, 0, 0, 0, 0],
        |output| {
            encode_linked_image_v3(
                &LinkedImageMetadata32V3 {
                    image: IMAGE,
                    gate_base: GATE_BASE,
                    gate_count: 2,
                    gates: GATES[..4].try_into().unwrap(),
                },
                output,
            )
        },
    );
    assert_linked_record(
        [b'R', b'3', b'L', b'I', 4, 0, 1, 0, 88, 0, 0, 0, 0, 0, 0, 0],
        |output| {
            encode_linked_image_v4(
                &LinkedImageMetadata32V4 {
                    image: IMAGE,
                    gate_base: GATE_BASE,
                    gate_count: 2,
                    gates: GATES[..5].try_into().unwrap(),
                },
                output,
            )
        },
    );
    assert_linked_record(
        [b'R', b'3', b'L', b'I', 5, 0, 1, 0, 96, 0, 0, 0, 0, 0, 0, 0],
        |output| {
            encode_linked_image_v5(
                &LinkedImageMetadata32V5 {
                    image: IMAGE,
                    gate_base: GATE_BASE,
                    gate_count: 2,
                    gates: GATES[..6].try_into().unwrap(),
                },
                output,
            )
        },
    );
    assert_linked_record(
        [b'R', b'3', b'L', b'I', 6, 0, 1, 0, 104, 0, 0, 0, 0, 0, 0, 0],
        |output| {
            encode_linked_image_v6(
                &LinkedImageMetadata32V6 {
                    image: IMAGE,
                    gate_base: GATE_BASE,
                    gate_count: 2,
                    gates: GATES[..7].try_into().unwrap(),
                },
                output,
            )
        },
    );
    assert_linked_record(
        [b'R', b'3', b'L', b'I', 7, 0, 1, 0, 112, 0, 0, 0, 0, 0, 0, 0],
        |output| {
            encode_linked_image_v7(
                &LinkedImageMetadata32V7 {
                    image: IMAGE,
                    gate_base: GATE_BASE,
                    gate_count: 2,
                    gates: GATES,
                },
                output,
            )
        },
    );
    assert_linked_record(
        [b'R', b'3', b'L', b'I', 8, 0, 1, 0, 112, 0, 0, 0, 0, 0, 0, 0],
        |output| {
            encode_linked_image_v8(
                &LinkedImageMetadata32V8 {
                    image: IMAGE,
                    gate_base: GATE_BASE,
                    gate_count: 2,
                    gates: GATES,
                },
                output,
            )
        },
    );
}
