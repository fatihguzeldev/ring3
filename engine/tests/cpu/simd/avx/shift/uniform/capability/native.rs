use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
};

const FORMS: &[(u8, u8, u32)] = &[
    (0x71, 2, 1),
    (0x71, 4, 2),
    (0x71, 6, 3),
    (0x72, 2, 4),
    (0x72, 4, 5),
    (0x72, 6, 6),
    (0x73, 2, 7),
    (0x73, 3, 8),
    (0x73, 6, 9),
    (0x73, 7, 10),
    (0xd1, 0, 11),
    (0xd2, 0, 12),
    (0xd3, 0, 13),
    (0xe1, 0, 14),
    (0xe2, 0, 15),
    (0xf1, 0, 16),
    (0xf2, 0, 17),
    (0xf3, 0, 18),
];
fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}
fn encoding((opcode, group, kind): (u8, u8, u32), wide: bool, w: bool, memory: bool) -> Vec<u8> {
    let immediate = kind <= 10;
    let vvvv = if immediate { 14 } else { 13 };
    let mut bytes = vec![
        0xc4,
        0xe1,
        vvvv << 3 | if wide { 4 } else { 0 } | 1 | if w { 0x80 } else { 0 },
        opcode,
    ];
    bytes.push(if immediate {
        0xc0 | group << 3 | 2
    } else if memory {
        0x08
    } else {
        0xcb
    });
    if immediate {
        bytes.push(255);
    }
    bytes
}
#[test]
fn ymm_uniform_encodings_stay_private_while_xmm_forms_remain_available() {
    for &form in FORMS {
        for w in [false, true] {
            for memory in [false, true] {
                if form.2 <= 10 && memory {
                    continue;
                }
                let bytes = encoding(form, true, w, memory);
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::Unsupported(UnsupportedFeature::Simd)
                );
                let xmm = encoding(form, false, w, memory);
                assert!(decode_one(&code(&xmm), GuestAddress(0x1000)).is_ok());
            }
        }
    }
}
#[test]
fn reserved_prefixes_are_rejected_before_uniform_capability_admission() {
    for &form in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(encoding(form, true, false, false));
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}
#[test]
fn canonical_wide_shift_helpers_refuse_without_state_or_ram_publication() {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let mut before_ram = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut before_ram)
        .unwrap();
    for &(_, _, kind) in FORMS {
        let roles = if kind <= 10 {
            vec![1 | 2 << 3]
        } else {
            vec![1 | 2 << 3 | 3 << 6, 1 | 2 << 3 | 1 << 12]
        };
        for operands in roles {
            engine.arena_mut().unwrap()[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
            let before = engine.arena().to_vec();
            assert_eq!(
                engine.avx_execute(
                    0x01000780 | kind,
                    operands,
                    if kind <= 10 { 255 } else { 0 },
                    0,
                    0,
                    2
                ),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
            let mut after_ram = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(0x1000), &mut after_ram)
                .unwrap();
            assert_eq!(after_ram, before_ram);
            assert_eq!(engine.arena(), before);
        }
    }
}
