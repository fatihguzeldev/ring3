use ring3_engine::process::EngineInstance;

#[test]
fn current_arena_appends_a_strict_zero_upper_record_without_moving_legacy_state() {
    let engine = EngineInstance::new(1, 91).unwrap();
    let arena = engine.arena();
    assert_eq!(
        arena.len(),
        4756,
        "current arena includes complete upper state"
    );
    assert_eq!(&arena[4364..4368], b"R3SV");
    assert_eq!(&arena[4524..4528], b"R3SG");
    assert_eq!(&arena[4588..4592], b"R3WK");
    assert_eq!(&arena[4612..4616], b"R3YU");
    assert_eq!(&arena[4616..4620], &[1, 0, 1, 0]);
    assert_eq!(&arena[4620..4624], &144_u32.to_le_bytes());
    assert!(arena[4624..4756].iter().all(|byte| *byte == 0));
}

#[test]
fn every_default_legacy_prefix_byte_remains_identical() {
    let arena = EngineInstance::new(1, 91).unwrap().arena().to_vec();
    let mut prefix = vec![0; 4612];
    for (offset, magic, size) in [
        (0, b"R3ST", 56),
        (56, b"R3EX", 40),
        (100, b"R3MH", 40),
        (4236, b"R3FP", 128),
        (4364, b"R3SV", 160),
        (4524, b"R3SG", 64),
        (4588, b"R3WK", 24),
    ] {
        prefix[offset..offset + 4].copy_from_slice(magic);
        prefix[offset + 4..offset + 8].copy_from_slice(&[1, 0, 1, 0]);
        prefix[offset + 8..offset + 12].copy_from_slice(&(size as u32).to_le_bytes());
    }
    prefix[52..56].copy_from_slice(&2_u32.to_le_bytes());
    prefix[72..76].copy_from_slice(&1_u32.to_le_bytes());
    prefix[4252..4254].copy_from_slice(&0x037f_u16.to_le_bytes());
    prefix[4256..4258].copy_from_slice(&0xffff_u16.to_le_bytes());
    prefix[4380..4384].copy_from_slice(&0x1f80_u32.to_le_bytes());
    for (index, selector) in [0x23_u16, 0x1b, 0x23, 0x23, 0x3b, 0]
        .into_iter()
        .enumerate()
    {
        prefix[4540 + index * 2..4542 + index * 2].copy_from_slice(&selector.to_le_bytes());
    }
    assert_eq!(&arena[..4612], prefix);
}

#[test]
fn upper_state_codec_keeps_raw_register_order_and_strict_version_identity() {
    use ring3_engine::{
        abi::{
            AbiError,
            x86::{YMM_SIZE, decode_ymm, encode_ymm},
        },
        cpu::x86::simd::ymm::YmmUpperState,
    };
    let state = YmmUpperState {
        registers: std::array::from_fn(|r| std::array::from_fn(|b| (r * 31 + b * 17) as u8)),
    };
    let mut bytes = [0x6d; YMM_SIZE];
    encode_ymm(&state, &mut bytes).unwrap();
    for (index, register) in state.registers.iter().enumerate() {
        assert_eq!(&bytes[16 + index * 16..32 + index * 16], register);
    }
    assert_eq!(decode_ymm(&bytes), Ok(state));
    for offset in 0..16 {
        let mut bad = bytes;
        bad[offset] ^= 0x80;
        assert!(decode_ymm(&bad).is_err());
    }
    assert_eq!(decode_ymm(&bytes[..143]), Err(AbiError::Length));
}
