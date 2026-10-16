use super::*;
use crate::input::dinput::{
    DirectInputResult32 as H,
    haptics::{EffectEnvelope32, EffectKind32},
};

fn constant() -> FileEffect32 {
    FileEffect32 {
        guid: EffectKind32::Constant.guid(),
        name: b"authored force".to_vec(),
        flags: 0x11,
        duration: 10000,
        sample_period: 0,
        gain: 8000,
        trigger: u32::MAX,
        trigger_repeat: u32::MAX,
        axes: vec![2, 0x102],
        direction: vec![1, 0],
        envelope: Some(EffectEnvelope32 {
            attack_level: 0,
            attack_time: 1000,
            fade_level: 0,
            fade_time: 1000,
        }),
        type_specific: 4000i32.to_le_bytes().to_vec(),
        opaque_chunks: Vec::new(),
        start_delay: 500,
    }
}
#[test]
fn native_ffe_layout_offsets_and_owned_standard_effect_round_trip_are_literal() {
    let expected = constant();
    let bytes = encode(std::slice::from_ref(&expected), 4096).unwrap();
    assert_eq!(bytes.len(), 252);
    assert_eq!(&bytes[..12], b"RIFF\0\0\0\0FORC");
    assert_eq!(&bytes[28..32], b"LIST");
    assert_eq!(u32::from_le_bytes(bytes[32..36].try_into().unwrap()), 216);
    assert_eq!(&bytes[36..40], b"efct");
    assert_eq!(&bytes[40..54], b"authored force");
    assert_eq!(u32::from_le_bytes(bytes[104..108].try_into().unwrap()), 56);
    assert_eq!(&bytes[160..176], &EffectKind32::Constant.guid());
    assert_eq!(&bytes[180..188], b"data\x08\0\0\0");
    assert_eq!(decode(&bytes, 2, 4096).unwrap(), vec![expected]);
}
#[test]
fn malformed_lengths_opaque_unknown_guid_and_budget_limits_are_distinguished() {
    let mut effect = constant();
    effect.guid = [0x51; 16];
    effect.type_specific = vec![1, 2, 3];
    effect.envelope = None;
    let bytes = encode(&[effect.clone()], 4096).unwrap();
    assert_eq!(decode(&bytes, 2, 4096).unwrap(), vec![effect]);
    assert_eq!(decode(&bytes, 0, 4096), Err(H::OUT_OF_MEMORY));
    for end in 0..bytes.len() {
        assert!(decode(&bytes[..end], 2, 4096).is_err() || end == 28)
    }
    let mut malformed = bytes.clone();
    malformed[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode(&malformed, 2, 4096), Err(H::INVALID_PARAM));
    assert_eq!(encode(&[constant()], 100), Err(H::OUT_OF_MEMORY));
}

#[test]
fn native_custom_file_metadata_round_trips_raw_pointer_without_embedded_samples() {
    for (input, expected) in [
        (
            &include_bytes!("fixtures/windows-custom-stereo.ffe")[..],
            [2, 1000, 8, 0x0133_fe54u32],
        ),
        (
            &include_bytes!("fixtures/windows-custom-mono.ffe")[..],
            [1, 7000, 4, 0x0133_fe54u32],
        ),
    ] {
        assert_eq!(input.len(), 236);
        assert_eq!(&input[212..220], b"data\x10\0\0\0");
        let effects = decode(input, 1, 236).unwrap();
        assert_eq!(effects[0].guid, EffectKind32::Custom.guid());
        assert_eq!(effects[0].type_specific.len(), 16);
        assert_eq!(
            effects[0]
                .type_specific
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                .collect::<Vec<_>>(),
            expected
        );
        assert!(effects[0].opaque_chunks.is_empty());
        assert_eq!(encode(&effects, 236).unwrap(), input);
    }
}

#[test]
fn custom_file_metadata_is_structurally_bounded_without_validating_the_sample_pointer() {
    let input = include_bytes!("fixtures/windows-custom-stereo.ffe");
    let mut effect = decode(input, 1, 236).unwrap().remove(0);
    effect.type_specific[8..]
        .copy_from_slice(&[u32::MAX, 0xffff_fffe].map(u32::to_le_bytes).concat());
    let encoded = encode(&[effect.clone()], 236).unwrap();
    assert_eq!(decode(&encoded, 1, 236).unwrap(), vec![effect.clone()]);
    effect.type_specific.push(0);
    assert_eq!(encode(&[effect], 4096), Err(H::INVALID_PARAM));
    assert_eq!(decode(input, 1, 235), Err(H::OUT_OF_MEMORY));
}

#[test]
fn unknown_vendor_riff_chunks_round_trip_opaquely_without_custom_force_interpretation() {
    let mut vendor = constant();
    vendor.guid = [0x71; 16];
    vendor.type_specific = vec![3, 4, 5];
    vendor.opaque_chunks = vec![
        OpaqueChunk32 {
            tag: *b"VEND",
            bytes: vec![9, 8, 7],
        },
        OpaqueChunk32 {
            tag: *b"data",
            bytes: vec![1, 2],
        },
    ];
    let bytes = encode(std::slice::from_ref(&vendor), 4096).unwrap();
    assert_eq!(decode(&bytes, 2, 4096).unwrap(), vec![vendor]);
    assert_eq!(&bytes[bytes.len() - 22..bytes.len() - 18], b"VEND");
}
