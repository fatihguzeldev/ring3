use super::*;
use crate::process::EngineInstance;

#[test]
fn prepared_capture_preserves_state_and_shadow_across_all_record_versions() {
    use crate::abi::x86::{encode_state, encode_state_v2, encode_state_v3};

    for (version, encode) in [encode_state, encode_state_v2, encode_state_v3]
        .into_iter()
        .enumerate()
    {
        let mut engine = EngineInstance::new(1, 71).unwrap();
        let state = State32 {
            registers: std::array::from_fn(|index| 0xfedc_ba98_u32.rotate_right(index as u32)),
            eip: 0x8765_4321,
            eflags: 2,
        };
        let metadata = StateMetadata {
            ss_debug_shadow: version != 0,
        };
        let arena = engine.arena_mut().unwrap();
        encode(&state, &mut arena[..STATE_SIZE]).unwrap();
        metadata.write((&mut arena[..STATE_SIZE]).try_into().unwrap());
        let before = engine.arena().to_vec();

        let capture = ExtendedCpuState::prepare_capture(engine.arena()).unwrap();
        assert_eq!(capture.state(), state);
        let extended = capture.finish().unwrap();
        assert_eq!(extended.metadata, metadata);
        assert!(extended.matches(engine.arena()));
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn validated_snapshots_preserve_every_opaque_vector_and_float_payload_bit() {
    let mut engine = EngineInstance::new(1, 71).unwrap();
    for (start, length) in [
        (X87_OFFSET + 40, 80),
        (SIMD_OFFSET + 32, 128),
        (YMM_OFFSET + 16, 128),
    ] {
        for offset in start..start + length {
            for bit in 0..8 {
                engine.arena_mut().unwrap()[offset] ^= 1 << bit;
                let captured = ExtendedCpuState::capture(engine.arena()).unwrap();
                assert!(captured.matches(engine.arena()));
                let mut restored = engine.arena().to_vec();
                restored[offset] ^= 1 << bit;
                captured.restore(&mut restored);
                assert_eq!(restored, engine.arena());
                engine.arena_mut().unwrap()[offset] ^= 1 << bit;
            }
        }
    }
}

#[test]
fn every_capture_revalidates_header_reserved_and_control_fields_after_a_valid_snapshot() {
    let mut engine = EngineInstance::new(1, 71).unwrap();
    let original = engine.arena().to_vec();
    for (offset, bit) in [
        (X87_OFFSET, 1),
        (X87_OFFSET + 6, 2),
        (X87_OFFSET + 23, 8),
        (X87_OFFSET + 36, 1),
        (X87_OFFSET + 127, 1),
        (SIMD_OFFSET + 8, 1),
        (SIMD_OFFSET + 18, 1),
        (SIMD_OFFSET + 24, 1),
        (YMM_OFFSET + 4, 1),
        (YMM_OFFSET + 12, 1),
    ] {
        ExtendedCpuState::capture(engine.arena()).unwrap();
        engine.arena_mut().unwrap()[offset] ^= bit;
        let malformed = engine.arena().to_vec();
        assert_eq!(
            ExtendedCpuState::capture(engine.arena()),
            Err(HostError::Call(CallError::InvalidStop))
        );
        assert_eq!(engine.arena(), malformed);
        engine.arena_mut().unwrap().copy_from_slice(&original);
    }
}
